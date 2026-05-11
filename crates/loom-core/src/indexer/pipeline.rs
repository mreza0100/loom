use crate::{
    embedder::{build_symbol_text, Embedder},
    error::{LoomError, Result},
    git_analyzer::{GitAnalyzer, SystemCommandRunner},
    indexer::{path, resolver::EdgeResolver, walk},
    models::Edge,
    parsers::{parse_file, AdapterRegistry, ParseResult},
    store::LoomDb,
    LoomConfig,
};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info, warn};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct IndexResult {
    pub indexed: usize,
    pub skipped: usize,
    pub deleted: usize,
    pub symbols: usize,
    pub edges: usize,
    pub embeddings: usize,
    pub resolved: usize,
    pub cochange_pairs: usize,
    pub errors: usize,
}

#[derive(Debug)]
struct ParsedFile {
    db_path: String,
    content_hash: String,
    parsed: ParseResult,
}

pub struct IndexPipeline<E: Embedder> {
    config: LoomConfig,
    db: Arc<LoomDb>,
    embedder: Arc<E>,
}

impl<E: Embedder> IndexPipeline<E> {
    pub fn new(config: LoomConfig, db: Arc<LoomDb>, embedder: Arc<E>) -> Self {
        Self {
            config,
            db,
            embedder,
        }
    }

    pub fn full_index(&self) -> Result<IndexResult> {
        let files = walk::discover_files(&self.config);
        let discovered = files
            .iter()
            .map(|file| path::db_path_for(file, &self.config))
            .collect::<Result<BTreeSet<_>>>()?;
        let mut result = self.remove_stale_files(&discovered)?;
        let indexed = self.index_paths(files)?;
        result.indexed += indexed.indexed;
        result.skipped += indexed.skipped;
        result.symbols += indexed.symbols;
        result.edges += indexed.edges;
        result.embeddings += indexed.embeddings;
        result.errors += indexed.errors;
        result.resolved = EdgeResolver::new(&self.db).resolve_all()?;
        if self.config.enable_git_analysis {
            let analyzer = GitAnalyzer::new(
                self.config.clone(),
                Arc::new(SystemCommandRunner),
                std::time::Duration::from_secs(30),
            );
            if analyzer.is_git_repo()? {
                let cochanges = analyzer.analyze_cochanges()?;
                for pair in &cochanges {
                    self.db.upsert_cochange_with_recency(
                        &pair.file_a,
                        &pair.file_b,
                        pair.frequency,
                        pair.recency,
                    )?;
                }
                result.cochange_pairs = cochanges.len();
            }
        }
        Ok(result)
    }

    pub fn incremental_index<I>(&self, changed_paths: I) -> Result<IndexResult>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut files = Vec::new();
        let mut result = IndexResult::default();
        for changed_path in changed_paths {
            let absolute = path::absolute_path(changed_path, &self.config);
            if absolute.exists() {
                let _ = path::db_path_for(&absolute, &self.config)?;
                if path::should_index(&absolute, &self.config) {
                    files.push(absolute);
                }
            } else if let Ok(db_path) = path::db_path_for(&absolute, &self.config) {
                self.db.remove_file(&db_path)?;
                result.deleted += 1;
            }
        }
        let indexed = self.index_paths(files)?;
        result.indexed += indexed.indexed;
        result.skipped += indexed.skipped;
        result.symbols += indexed.symbols;
        result.edges += indexed.edges;
        result.embeddings += indexed.embeddings;
        result.errors += indexed.errors;
        result.resolved = EdgeResolver::new(&self.db).resolve_all()?;
        Ok(result)
    }

    fn index_paths(&self, files: Vec<PathBuf>) -> Result<IndexResult> {
        let mut result = IndexResult::default();
        let mut jobs = Vec::new();
        for file in files {
            let db_path = path::db_path_for(&file, &self.config)?;
            let content_hash = walk::hash_file(&file)?;
            if self.db.get_file_hash(&db_path)?.as_deref() == Some(content_hash.as_str()) {
                result.skipped += 1;
                continue;
            }
            jobs.push((file, db_path, content_hash));
        }
        if jobs.is_empty() {
            return Ok(result);
        }

        let mut parsed_files = Vec::new();
        for parsed in jobs
            .into_par_iter()
            .map(|(absolute_path, db_path, content_hash)| {
                parse_one_file(absolute_path, db_path, content_hash)
            })
            .collect::<Vec<_>>()
        {
            match parsed {
                Ok(parsed_file) => parsed_files.push(parsed_file),
                Err(source) => {
                    result.errors += 1;
                    error!(error = %source, "failed to parse file during index");
                }
            }
        }

        let embedded_files = self.embed_parsed_files(&parsed_files)?;
        result.embeddings = embedded_files.iter().map(Vec::len).sum();
        for (parsed_file, embeddings) in parsed_files.into_iter().zip(embedded_files) {
            let (symbol_count, edge_count) = self.write_parsed_file(parsed_file, embeddings)?;
            result.indexed += 1;
            result.symbols += symbol_count;
            result.edges += edge_count;
        }
        info!(
            indexed = result.indexed,
            symbols = result.symbols,
            edges = result.edges,
            embeddings = result.embeddings,
            errors = result.errors,
            "index phase complete"
        );
        Ok(result)
    }

    fn remove_stale_files(&self, discovered: &BTreeSet<String>) -> Result<IndexResult> {
        let mut result = IndexResult::default();
        for indexed_file in self.db.list_indexed_files()? {
            if !discovered.contains(&indexed_file) {
                self.db.remove_file(&indexed_file)?;
                result.deleted += 1;
                warn!(file = indexed_file, "removed stale file from index");
            }
        }
        Ok(result)
    }

    fn write_parsed_file(
        &self,
        parsed_file: ParsedFile,
        embeddings: Vec<Vec<f32>>,
    ) -> Result<(usize, usize)> {
        let mut symbols = parsed_file.parsed.symbols;
        if embeddings.len() != symbols.len() {
            return Err(LoomError::EmbedderModel(format!(
                "embedder returned {} vectors for {} symbols",
                embeddings.len(),
                symbols.len()
            )));
        }
        for symbol in &mut symbols {
            symbol.file = parsed_file.db_path.clone();
        }
        let edge_count = parsed_file.parsed.edges.len();
        let db_path = parsed_file.db_path.clone();
        let parsed_edges = parsed_file.parsed.edges;
        let (symbol_count, edge_count) = self.db.replace_file_index(
            &parsed_file.db_path,
            &parsed_file.content_hash,
            &symbols,
            &embeddings,
            |symbol_ids| {
                let mut local_name_to_id = BTreeMap::new();
                for (symbol, symbol_id) in symbols.iter().zip(symbol_ids.iter()) {
                    local_name_to_id.insert(symbol.name.clone(), *symbol_id);
                }

                let file_anchor_id = symbol_ids.first().copied();
                let mut edges = Vec::with_capacity(edge_count);
                for parsed in &parsed_edges {
                    if parsed.relationship == "imports" {
                        let Some(source_id) = file_anchor_id else {
                            continue;
                        };
                        let target_file = parsed.target_file.as_ref().map(|target_file| {
                            if target_file.starts_with('.') {
                                path::resolve_import_path(target_file, &db_path)
                            } else {
                                target_file.clone()
                            }
                        });
                        edges.push(Edge {
                            id: None,
                            source_id,
                            target_id: None,
                            target_name: parsed.source_name.clone(),
                            target_file,
                            relationship: "imports".to_string(),
                            confidence: 0.0,
                            original_name: (parsed.target_name != parsed.source_name)
                                .then(|| parsed.target_name.clone()),
                        });
                        continue;
                    }
                    let Some(source_id) = local_name_to_id.get(&parsed.source_name).copied() else {
                        continue;
                    };
                    edges.push(Edge {
                        id: None,
                        source_id,
                        target_id: None,
                        target_name: parsed.target_name.clone(),
                        target_file: parsed.target_file.clone(),
                        relationship: parsed.relationship.clone(),
                        confidence: 0.0,
                        original_name: None,
                    });
                }
                Ok(edges)
            },
        )?;
        Ok((symbol_count, edge_count))
    }

    fn embed_parsed_files(&self, parsed_files: &[ParsedFile]) -> Result<Vec<Vec<Vec<f32>>>> {
        let mut symbol_counts = Vec::with_capacity(parsed_files.len());
        let mut texts = Vec::new();
        for parsed_file in parsed_files {
            symbol_counts.push(parsed_file.parsed.symbols.len());
            texts.extend(
                parsed_file
                    .parsed
                    .symbols
                    .iter()
                    .map(|symbol| build_symbol_text(&symbol.name, &symbol.kind, &symbol.context)),
            );
        }

        let mut embeddings = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(128) {
            let chunk_texts = chunk.to_vec();
            let chunk_embeddings = self.embedder.embed(&chunk_texts)?;
            if chunk_embeddings.len() != chunk.len() {
                return Err(LoomError::EmbedderModel(format!(
                    "embedder returned {} vectors for {} texts",
                    chunk_embeddings.len(),
                    chunk.len()
                )));
            }
            embeddings.extend(chunk_embeddings);
        }

        let mut by_file = Vec::with_capacity(symbol_counts.len());
        let mut embedded_iter = embeddings.into_iter();
        for count in symbol_counts {
            by_file.push(embedded_iter.by_ref().take(count).collect());
        }
        Ok(by_file)
    }
}

fn parse_one_file(
    absolute_path: PathBuf,
    db_path: String,
    content_hash: String,
) -> Result<ParsedFile> {
    let bytes = fs::read(&absolute_path).map_err(|source| LoomError::IndexerIo {
        path: absolute_path.display().to_string(),
        source,
    })?;
    let registry = AdapterRegistry::with_builtin_adapters();
    let parsed = parse_file(&absolute_path, Some(&bytes), &registry)?;
    Ok(ParsedFile {
        db_path,
        content_hash,
        parsed,
    })
}
