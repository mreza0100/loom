use crate::{
    embedder::{build_symbol_text, Embedder},
    error::{LoomError, Result},
    git_analyzer::{GitAnalyzer, SystemCommandRunner},
    indexer::{path, resolver::EdgeResolver, walk},
    models::{AliasRecord, BehaviorFact, Callsite, Edge, FileRoleCard},
    parsers::{parse_file, AdapterRegistry, ParseResult},
    store::{FileIndexReplacement, LoomDb},
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
    pub behavior_facts: usize,
    pub callsites: usize,
    pub aliases: usize,
    pub role_cards: usize,
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
        result.behavior_facts += indexed.behavior_facts;
        result.callsites += indexed.callsites;
        result.aliases += indexed.aliases;
        result.role_cards += indexed.role_cards;
        result.embeddings += indexed.embeddings;
        result.errors += indexed.errors;
        result.resolved = self.resolve_index_signals()?;
        if self.config.enable_git_analysis {
            let analyzer = GitAnalyzer::new(
                self.config.clone(),
                Arc::new(SystemCommandRunner),
                std::time::Duration::from_secs(30),
            );
            if analyzer.is_git_repo()? {
                let cochanges = analyzer.analyze_cochanges()?;
                let rows = cochanges
                    .iter()
                    .map(|pair| {
                        (
                            pair.file_a.clone(),
                            pair.file_b.clone(),
                            pair.frequency,
                            pair.recency,
                        )
                    })
                    .collect::<Vec<_>>();
                self.db.replace_cochanges(&rows)?;
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
        result.behavior_facts += indexed.behavior_facts;
        result.callsites += indexed.callsites;
        result.aliases += indexed.aliases;
        result.role_cards += indexed.role_cards;
        result.embeddings += indexed.embeddings;
        result.errors += indexed.errors;
        result.resolved = self.resolve_index_signals()?;
        Ok(result)
    }

    fn index_paths(&self, files: Vec<PathBuf>) -> Result<IndexResult> {
        let mut result = IndexResult::default();
        let mut jobs = Vec::new();
        let embedding_fingerprint = self.embedder.fingerprint();
        for file in files {
            let db_path = path::db_path_for(&file, &self.config)?;
            let content_hash = walk::hash_file(&file)?;
            if self
                .db
                .file_index_is_fresh(&db_path, &content_hash, &embedding_fingerprint)?
            {
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
            let (symbol_count, edge_count, fact_count, callsite_count, alias_count) =
                self.write_parsed_file(parsed_file, embeddings)?;
            result.indexed += 1;
            result.symbols += symbol_count;
            result.edges += edge_count;
            result.behavior_facts += fact_count;
            result.callsites += callsite_count;
            result.aliases += alias_count;
            result.role_cards += 1;
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

    fn resolve_index_signals(&self) -> Result<usize> {
        let edge_resolutions = EdgeResolver::new(&self.db).resolve_all()?;
        let _enclosed = self.db.resolve_signal_enclosures()?;
        let callsite_resolutions = self.db.resolve_callsites_from_edges()?;
        self.db.refresh_role_cards()?;
        Ok(edge_resolutions + callsite_resolutions)
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
    ) -> Result<(usize, usize, usize, usize, usize)> {
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
        let behavior_facts =
            materialize_behavior_facts(&db_path, parsed_file.parsed.behavior_facts);
        let callsites = materialize_callsites(&db_path, parsed_file.parsed.callsites);
        let aliases = materialize_aliases(&db_path, parsed_file.parsed.aliases);
        let role_card = build_role_card(
            &db_path,
            &parsed_file.content_hash,
            &symbols,
            &parsed_edges,
            &behavior_facts,
            &aliases,
        );
        let fact_count = behavior_facts.len();
        let callsite_count = callsites.len();
        let alias_count = aliases.len();
        let embedding_fingerprint = self.embedder.fingerprint();
        let replacement = FileIndexReplacement {
            path: &parsed_file.db_path,
            content_hash: &parsed_file.content_hash,
            symbols: &symbols,
            embeddings: &embeddings,
            embedding_fingerprint: &embedding_fingerprint,
            behavior_facts: &behavior_facts,
            callsites: &callsites,
            aliases: &aliases,
            role_card: &role_card,
        };
        let (symbol_count, edge_count) = self.db.replace_file_index(replacement, |symbol_ids| {
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
        })?;
        Ok((
            symbol_count,
            edge_count,
            fact_count,
            callsite_count,
            alias_count,
        ))
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

fn materialize_behavior_facts(
    file: &str,
    facts: Vec<crate::models::ParsedBehaviorFact>,
) -> Vec<BehaviorFact> {
    let mut aggregated = BTreeMap::<(String, String, Option<String>), (i64, i64, i64)>::new();
    for fact in facts {
        let key = (
            fact.fact_type,
            fact.value,
            fact.enclosing_symbol_name.clone(),
        );
        aggregated
            .entry(key)
            .and_modify(|(line, end_line, count)| {
                *line = (*line).min(fact.line);
                *end_line = (*end_line).max(fact.end_line);
                *count += 1;
            })
            .or_insert((fact.line, fact.end_line, 1));
    }
    aggregated
        .into_iter()
        .map(
            |((fact_type, value, enclosing_symbol_name), (line, end_line, count))| BehaviorFact {
                id: None,
                fact_type,
                value,
                file: file.to_string(),
                line,
                end_line,
                enclosing_symbol_id: None,
                enclosing_symbol_name,
                occurrence_count: count,
            },
        )
        .collect()
}

fn materialize_callsites(
    file: &str,
    callsites: Vec<crate::models::ParsedCallsite>,
) -> Vec<Callsite> {
    callsites
        .into_iter()
        .map(|callsite| Callsite {
            id: None,
            file: file.to_string(),
            line: callsite.line,
            end_line: callsite.end_line,
            callee: callsite.callee,
            receiver: callsite.receiver,
            unresolved_target: callsite.unresolved_target,
            resolved_target_id: None,
            argument_summaries: callsite.argument_summaries,
            imported_aliases: callsite.imported_aliases,
            enclosing_symbol_id: None,
            enclosing_symbol_name: callsite.enclosing_symbol_name,
            confidence: callsite.confidence,
            generic: callsite.generic,
            downweighted: callsite.downweighted,
        })
        .collect()
}

fn materialize_aliases(file: &str, aliases: Vec<crate::models::ParsedAlias>) -> Vec<AliasRecord> {
    aliases
        .into_iter()
        .map(|alias| AliasRecord {
            id: None,
            file: file.to_string(),
            line: alias.line,
            end_line: alias.end_line,
            local_name: alias.local_name,
            imported_name: alias.imported_name,
            source: alias.source,
            alias_kind: alias.alias_kind,
            enclosing_symbol_id: None,
            enclosing_symbol_name: alias.enclosing_symbol_name,
        })
        .collect()
}

fn build_role_card(
    file: &str,
    content_hash: &str,
    symbols: &[crate::models::Symbol],
    edges: &[crate::models::ParsedEdge],
    facts: &[BehaviorFact],
    aliases: &[AliasRecord],
) -> FileRoleCard {
    let exported_symbols = symbols
        .iter()
        .map(|symbol| format!("{}:{}", symbol.kind, symbol.name))
        .take(16)
        .collect::<Vec<_>>();
    let mut imported_dependencies = edges
        .iter()
        .filter(|edge| edge.relationship == "imports")
        .filter_map(|edge| {
            edge.target_file
                .clone()
                .or_else(|| Some(edge.target_name.clone()))
        })
        .chain(aliases.iter().map(|alias| alias.source.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(16)
        .collect::<Vec<_>>();
    imported_dependencies.sort();
    let behavior_facts = facts
        .iter()
        .map(|fact| format!("{}:{}", fact.fact_type, fact.value))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(16)
        .collect::<Vec<_>>();
    let primary_responsibility = symbols
        .iter()
        .find(|symbol| matches!(symbol.kind.as_str(), "class" | "function" | "method"))
        .map(|symbol| format!("{} {}", symbol.kind, symbol.name))
        .or_else(|| {
            facts
                .iter()
                .find(|fact| fact.fact_type == "package_name")
                .map(|fact| format!("package manifest {}", fact.value))
        })
        .unwrap_or_else(|| format!("indexed facts for {file}"));
    let tests_touching = if is_test_file(file) {
        vec![file.to_string()]
    } else {
        Vec::new()
    };
    FileRoleCard {
        file: file.to_string(),
        content_hash: content_hash.to_string(),
        primary_responsibility,
        exported_symbols,
        imported_dependencies,
        behavior_facts,
        centrality: 0.0,
        tests_touching,
        top_related_files: Vec::new(),
    }
}

fn is_test_file(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    lower.contains("test") || lower.contains("spec")
}
