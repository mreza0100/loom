pub mod migrations;
pub mod vector;

use crate::config::{LoomConfig, VectorBackendConfig};
use crate::error::{LoomError, Result};
use crate::models::{
    AliasRecord, BehaviorFact, BehaviorFactHit, Callsite, Edge, FileRoleCard, FtsSearchResult,
    LexicalEvidence, StoreStats, Symbol,
};
use migrations::run_migrations;
use parking_lot::Mutex;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;
use vector::{
    register_sqlite_vec_once, repeat_placeholders, BlobVectorStore, SqliteVecStore, VectorStore,
};

const FTS5_SPECIAL: [&str; 4] = ["AND", "OR", "NOT", "NEAR"];
const FTS_MATCH_START: &str = "[[";
const FTS_MATCH_END: &str = "]]";
const MAX_FTS_SNIPPET_CHARS: usize = 240;
const MAX_SQL_LIMIT: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderPragma {
    ForeignKeys,
    JournalMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportEdgeRow {
    pub local_name: String,
    pub source_file: String,
    pub target_file: String,
    pub original_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CochangeRow {
    pub file_a: String,
    pub file_b: String,
    pub frequency: i64,
    pub recency: f64,
}

pub struct FileIndexReplacement<'a> {
    pub path: &'a str,
    pub content_hash: &'a str,
    pub symbols: &'a [Symbol],
    pub embeddings: &'a [Vec<f32>],
    pub embedding_fingerprint: &'a str,
    pub behavior_facts: &'a [BehaviorFact],
    pub callsites: &'a [Callsite],
    pub aliases: &'a [AliasRecord],
    pub role_card: &'a FileRoleCard,
}

impl ReaderPragma {
    #[must_use]
    const fn sql(self) -> &'static str {
        match self {
            Self::ForeignKeys => "PRAGMA foreign_keys",
            Self::JournalMode => "PRAGMA journal_mode",
        }
    }
}

pub struct LoomDb {
    config: LoomConfig,
    db_path: PathBuf,
    writer: Mutex<Connection>,
    readers: Pool<SqliteConnectionManager>,
    vector_store: Arc<dyn VectorStore>,
}

impl LoomDb {
    pub fn open(config: LoomConfig) -> Result<Self> {
        let db_path = config.resolve_db_path()?;
        let vector_store: Arc<dyn VectorStore> = match config.vector_backend {
            VectorBackendConfig::SqliteVec => {
                register_sqlite_vec_once()?;
                Arc::new(SqliteVecStore)
            }
            VectorBackendConfig::Blob => Arc::new(BlobVectorStore),
        };
        let writer = Connection::open(&db_path)?;
        apply_pragmas(&writer)?;

        let manager = SqliteConnectionManager::file(&db_path);
        let pool_size = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(2)
            .clamp(2, 16) as u32;
        let readers = Pool::builder().max_size(pool_size).build(manager)?;

        let db = Self {
            config,
            db_path,
            writer: Mutex::new(writer),
            readers,
            vector_store,
        };
        {
            let conn = db.writer.lock();
            run_migrations(&conn, &*db.vector_store, db.config.embedding_dimensions)?;
        }
        debug!(
            db_path = %db.db_path.display(),
            vector_backend = db.vector_store.backend_name(),
            "LoomDb schema initialized"
        );
        Ok(db)
    }

    #[must_use]
    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    #[must_use]
    pub fn vector_backend_name(&self) -> &'static str {
        self.vector_store.backend_name()
    }

    pub fn schema_version(&self) -> Result<i64> {
        let conn = self.reader()?;
        migrations::schema_version(&conn)
    }

    pub fn insert_symbol(&self, symbol: &Symbol) -> Result<i64> {
        let conn = self.writer.lock();
        let mut stmt = conn.prepare(
            "INSERT INTO symbols (name, kind, file, line, end_line, language, context)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )?;
        stmt.execute(params![
            symbol.name,
            symbol.kind,
            symbol.file,
            symbol.line,
            symbol.end_line,
            symbol.language,
            symbol.context
        ])?;
        let symbol_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO symbols_fts (rowid, name, kind, file, context) VALUES (?, ?, ?, ?, ?)",
            params![
                symbol_id,
                symbol.name,
                symbol.kind,
                symbol.file,
                symbol.context
            ],
        )?;
        Ok(symbol_id)
    }

    pub fn insert_symbols(&self, symbols: &[Symbol]) -> Result<Vec<i64>> {
        let mut conn = self.writer.lock();
        let tx = conn.transaction()?;
        let mut ids = Vec::with_capacity(symbols.len());
        {
            let mut symbol_stmt = tx.prepare(
                "INSERT INTO symbols (name, kind, file, line, end_line, language, context)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )?;
            let mut fts_stmt = tx.prepare(
                "INSERT INTO symbols_fts (rowid, name, kind, file, context)
                 VALUES (?, ?, ?, ?, ?)",
            )?;
            for symbol in symbols {
                symbol_stmt.execute(params![
                    symbol.name,
                    symbol.kind,
                    symbol.file,
                    symbol.line,
                    symbol.end_line,
                    symbol.language,
                    symbol.context
                ])?;
                let symbol_id = tx.last_insert_rowid();
                fts_stmt.execute(params![
                    symbol_id,
                    symbol.name,
                    symbol.kind,
                    symbol.file,
                    symbol.context
                ])?;
                ids.push(symbol_id);
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    pub fn insert_edge(&self, edge: &Edge) -> Result<i64> {
        let conn = self.writer.lock();
        conn.execute(
            "INSERT INTO edges
             (source_id, target_id, target_name, target_file, relationship, confidence, original_name)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                edge.source_id,
                edge.target_id,
                edge.target_name,
                edge.target_file,
                edge.relationship,
                edge.confidence,
                edge.original_name
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn insert_edges(&self, edges: &[Edge]) -> Result<Vec<i64>> {
        let mut conn = self.writer.lock();
        let tx = conn.transaction()?;
        let mut ids = Vec::with_capacity(edges.len());
        {
            let mut stmt = tx.prepare(
                "INSERT INTO edges
                 (source_id, target_id, target_name, target_file, relationship, confidence, original_name)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )?;
            for edge in edges {
                stmt.execute(params![
                    edge.source_id,
                    edge.target_id,
                    edge.target_name,
                    edge.target_file,
                    edge.relationship,
                    edge.confidence,
                    edge.original_name
                ])?;
                ids.push(tx.last_insert_rowid());
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    pub fn insert_embedding(&self, symbol_id: i64, embedding: &[f32]) -> Result<()> {
        let conn = self.writer.lock();
        self.vector_store.insert_embedding(
            &conn,
            symbol_id,
            embedding,
            self.config.embedding_dimensions,
        )
    }

    pub fn insert_embeddings(&self, embeddings: &[(i64, Vec<f32>)]) -> Result<()> {
        let mut conn = self.writer.lock();
        let tx = conn.transaction()?;
        for (symbol_id, embedding) in embeddings {
            self.vector_store.insert_embedding(
                &tx,
                *symbol_id,
                embedding,
                self.config.embedding_dimensions,
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn replace_file_index<F>(
        &self,
        replacement: FileIndexReplacement<'_>,
        build_edges: F,
    ) -> Result<(usize, usize)>
    where
        F: FnOnce(&[i64]) -> Result<Vec<Edge>>,
    {
        let FileIndexReplacement {
            path,
            content_hash,
            symbols,
            embeddings,
            embedding_fingerprint,
            behavior_facts,
            callsites,
            aliases,
            role_card,
        } = replacement;

        if embeddings.len() != symbols.len() {
            return Err(LoomError::EmbedderModel(format!(
                "embedder returned {} vectors for {} symbols",
                embeddings.len(),
                symbols.len()
            )));
        }

        let mut conn = self.writer.lock();
        let tx = conn.transaction()?;
        remove_file_in_transaction(&tx, &*self.vector_store, path)?;

        let mut symbol_ids = Vec::with_capacity(symbols.len());
        {
            let mut symbol_stmt = tx.prepare(
                "INSERT INTO symbols (name, kind, file, line, end_line, language, context)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )?;
            let mut fts_stmt = tx.prepare(
                "INSERT INTO symbols_fts (rowid, name, kind, file, context)
                 VALUES (?, ?, ?, ?, ?)",
            )?;
            for symbol in symbols {
                symbol_stmt.execute(params![
                    symbol.name,
                    symbol.kind,
                    symbol.file,
                    symbol.line,
                    symbol.end_line,
                    symbol.language,
                    symbol.context
                ])?;
                let symbol_id = tx.last_insert_rowid();
                fts_stmt.execute(params![
                    symbol_id,
                    symbol.name,
                    symbol.kind,
                    symbol.file,
                    symbol.context
                ])?;
                symbol_ids.push(symbol_id);
            }
        }

        let edges = build_edges(&symbol_ids)?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO edges
                 (source_id, target_id, target_name, target_file, relationship, confidence, original_name)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )?;
            for edge in &edges {
                stmt.execute(params![
                    edge.source_id,
                    edge.target_id,
                    edge.target_name,
                    edge.target_file,
                    edge.relationship,
                    edge.confidence,
                    edge.original_name
                ])?;
            }
        }

        insert_behavior_facts_in_transaction(&tx, behavior_facts)?;
        insert_callsites_in_transaction(&tx, callsites)?;
        insert_aliases_in_transaction(&tx, aliases)?;
        upsert_role_card_in_transaction(&tx, role_card)?;

        for (symbol_id, embedding) in symbol_ids.iter().zip(embeddings.iter()) {
            self.vector_store.insert_embedding(
                &tx,
                *symbol_id,
                embedding,
                self.config.embedding_dimensions,
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO index_meta
             (file_path, content_hash, embedding_fingerprint)
             VALUES (?, ?, ?)",
            params![path, content_hash, embedding_fingerprint],
        )?;
        tx.commit()?;
        Ok((symbols.len(), edges.len()))
    }

    pub fn search_fts_with_evidence(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FtsSearchResult>> {
        let sanitized = sanitize_fts_query(query);
        if sanitized.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let limit = checked_sql_limit(limit)?;
        let match_kind = classify_fts_match(query);
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.kind, s.file, s.line, s.end_line, s.language, s.context,
                    bm25(symbols_fts) AS lexical_rank,
                    snippet(symbols_fts, 0, '[[', ']]', ' ... ', 16) AS name_snippet,
                    snippet(symbols_fts, 1, '[[', ']]', ' ... ', 16) AS kind_snippet,
                    snippet(symbols_fts, 2, '[[', ']]', ' ... ', 16) AS file_snippet,
                    snippet(symbols_fts, 3, '[[', ']]', ' ... ', 16) AS context_snippet
             FROM symbols_fts
             JOIN symbols s ON s.id = symbols_fts.rowid
             WHERE symbols_fts MATCH ?
             ORDER BY lexical_rank, s.file, s.line, s.name, s.id LIMIT ?",
        )?;
        let rows = stmt.query_map(params![sanitized, limit], |row| {
            row_to_fts_result(row, &sanitized, &match_kind)
        })?;
        collect_rows(rows)
    }

    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<Symbol>> {
        Ok(self
            .search_fts_with_evidence(query, limit)?
            .into_iter()
            .map(|result| result.symbol)
            .collect())
    }

    pub fn search_behavior_facts(&self, query: &str, limit: usize) -> Result<Vec<BehaviorFactHit>> {
        let sanitized = sanitize_fts_query(query);
        if sanitized.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let limit = checked_sql_limit(limit)?;
        let match_kind = classify_fts_match(query);
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT f.id, f.fact_type, f.value, f.file, f.line, f.end_line,
                    f.enclosing_symbol_id, f.enclosing_symbol_name, f.occurrence_count,
                    bm25(behavior_facts_fts) AS lexical_rank,
                    snippet(behavior_facts_fts, 0, '[[', ']]', ' ... ', 16) AS type_snippet,
                    snippet(behavior_facts_fts, 1, '[[', ']]', ' ... ', 16) AS value_snippet,
                    snippet(behavior_facts_fts, 2, '[[', ']]', ' ... ', 16) AS file_snippet
             FROM behavior_facts_fts
             JOIN behavior_facts f ON f.id = behavior_facts_fts.rowid
             WHERE behavior_facts_fts MATCH ?
             ORDER BY lexical_rank, f.file, f.line, f.fact_type, f.value, f.id LIMIT ?",
        )?;
        let rows = stmt.query_map(params![sanitized, limit], |row| {
            row_to_behavior_fact_hit(row, &sanitized, &match_kind)
        })?;
        collect_rows(rows)
    }

    pub fn get_behavior_facts_for_file(
        &self,
        file: &str,
        limit: usize,
    ) -> Result<Vec<BehaviorFact>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = checked_sql_limit(limit)?;
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT id, fact_type, value, file, line, end_line,
                    enclosing_symbol_id, enclosing_symbol_name, occurrence_count
             FROM behavior_facts WHERE file = ?
             ORDER BY line, fact_type, value, id LIMIT ?",
        )?;
        let rows = stmt.query_map(params![file, limit], row_to_behavior_fact)?;
        collect_rows(rows)
    }

    pub fn get_callsites_for_file(&self, file: &str) -> Result<Vec<Callsite>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT id, file, line, end_line, callee, receiver, unresolved_target,
                    resolved_target_id, argument_summaries, imported_aliases,
                    enclosing_symbol_id, enclosing_symbol_name, confidence, generic, downweighted
             FROM callsites WHERE file = ?
             ORDER BY line, unresolved_target, id",
        )?;
        let rows = stmt.query_map([file], row_to_callsite)?;
        collect_rows(rows)
    }

    pub fn get_aliases_for_file(&self, file: &str) -> Result<Vec<AliasRecord>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT id, file, line, end_line, local_name, imported_name, source,
                    alias_kind, enclosing_symbol_id, enclosing_symbol_name
             FROM aliases WHERE file = ?
             ORDER BY line, local_name, imported_name, id",
        )?;
        let rows = stmt.query_map([file], row_to_alias)?;
        collect_rows(rows)
    }

    pub fn get_role_card(&self, file: &str) -> Result<Option<FileRoleCard>> {
        let conn = self.reader()?;
        conn.query_row(
            "SELECT file, content_hash, primary_responsibility, exported_symbols,
                    imported_dependencies, behavior_facts, centrality, tests_touching,
                    top_related_files
             FROM file_role_cards WHERE file = ?",
            [file],
            row_to_role_card,
        )
        .optional()
        .map_err(LoomError::from)
    }

    pub fn get_role_cards_for_files(&self, files: &[String]) -> Result<Vec<FileRoleCard>> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let mut cards = Vec::new();
        for file in files.iter().take(MAX_SQL_LIMIT) {
            if let Some(card) = self.get_role_card(file)? {
                cards.push(card);
            }
        }
        cards.sort_by(|left, right| left.file.cmp(&right.file));
        cards.dedup_by(|left, right| left.file == right.file);
        Ok(cards)
    }

    pub fn search_vectors(&self, embedding: &[f32], limit: usize) -> Result<Vec<(i64, f64)>> {
        let conn = self.reader()?;
        self.vector_store
            .search(&conn, embedding, self.config.embedding_dimensions, limit)
    }

    pub fn get_symbol_by_id(&self, symbol_id: i64) -> Result<Option<Symbol>> {
        let conn = self.reader()?;
        conn.query_row(
            "SELECT id, name, kind, file, line, end_line, language, context
             FROM symbols WHERE id = ?",
            [symbol_id],
            row_to_symbol,
        )
        .optional()
        .map_err(LoomError::from)
    }

    pub fn get_symbol_by_name(&self, name: &str, file: Option<&str>) -> Result<Vec<Symbol>> {
        let conn = self.reader()?;
        let mut rows = if let Some(file) = file {
            let mut stmt = conn.prepare(
                "SELECT id, name, kind, file, line, end_line, language, context
                 FROM symbols WHERE name = ? AND file = ?",
            )?;
            let rows = stmt.query_map(params![name, file], row_to_symbol)?;
            return collect_rows(rows);
        } else {
            conn.prepare(
                "SELECT id, name, kind, file, line, end_line, language, context
                 FROM symbols WHERE name = ?",
            )?
        };
        let rows = rows.query_map([name], row_to_symbol)?;
        collect_rows(rows)
    }

    pub fn get_symbol_by_name_fuzzy(&self, name: &str, file: Option<&str>) -> Result<Vec<Symbol>> {
        let results = self.get_symbol_by_name(name, file)?;
        if !results.is_empty() {
            return Ok(results);
        }

        if let Some(file) = file {
            let file_results = self.get_symbol_by_name(name, None)?;
            let matched = filter_file_suffix(file_results, file);
            if !matched.is_empty() {
                return Ok(matched);
            }
        }

        if !name.contains('.') {
            let method_results = self.method_suffix_lookup(name, file)?;
            if !method_results.is_empty() {
                return Ok(method_results);
            }
        }

        let alt_name = if name.starts_with('_') {
            name.trim_start_matches('_').to_string()
        } else {
            format!("_{name}")
        };
        if alt_name != name {
            let results = self.get_symbol_by_name(&alt_name, file)?;
            if !results.is_empty() {
                return Ok(results);
            }
            if let Some(file) = file {
                let file_results = self.get_symbol_by_name(&alt_name, None)?;
                let matched = filter_file_suffix(file_results, file);
                if !matched.is_empty() {
                    return Ok(matched);
                }
            }
            if !alt_name.contains('.') {
                let method_results = self.method_suffix_lookup(&alt_name, None)?;
                if !method_results.is_empty() {
                    return Ok(method_results);
                }
            }
        }

        Ok(Vec::new())
    }

    pub fn get_colocated_symbols(&self, file: &str) -> Result<Vec<Symbol>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, file, line, end_line, language, context
             FROM symbols WHERE file = ? ORDER BY line",
        )?;
        let rows = stmt.query_map([file], row_to_symbol)?;
        collect_rows(rows)
    }

    pub fn remove_file(&self, path: &str) -> Result<()> {
        let mut conn = self.writer.lock();
        let tx = conn.transaction()?;
        remove_file_in_transaction(&tx, &*self.vector_store, path)?;
        tx.commit()?;
        Ok(())
    }

    pub fn clear_index(&self) -> Result<()> {
        let mut conn = self.writer.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM behavior_facts_fts", [])?;
        tx.execute("DELETE FROM behavior_facts", [])?;
        tx.execute("DELETE FROM callsites", [])?;
        tx.execute("DELETE FROM aliases", [])?;
        tx.execute("DELETE FROM file_role_cards", [])?;
        tx.execute("DELETE FROM edges", [])?;
        tx.execute("DELETE FROM symbols_fts", [])?;
        tx.execute("DELETE FROM symbols", [])?;
        tx.execute("DELETE FROM index_meta", [])?;
        tx.execute("DELETE FROM cochange", [])?;
        self.vector_store.clear(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_edges_from(&self, symbol_id: i64) -> Result<Vec<Edge>> {
        self.query_edges(
            "SELECT id, source_id, target_id, target_name, target_file, relationship, confidence, original_name
             FROM edges WHERE source_id = ?",
            symbol_id,
        )
    }

    pub fn get_edges_to(&self, symbol_id: i64) -> Result<Vec<Edge>> {
        self.query_edges(
            "SELECT id, source_id, target_id, target_name, target_file, relationship, confidence, original_name
             FROM edges WHERE target_id = ?",
            symbol_id,
        )
    }

    pub fn get_edges_to_by_name(&self, target_name: &str) -> Result<Vec<Edge>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT id, source_id, target_id, target_name, target_file, relationship, confidence, original_name
             FROM edges WHERE target_name = ?",
        )?;
        let rows = stmt.query_map([target_name], row_to_edge)?;
        collect_rows(rows)
    }

    pub fn get_unresolved_edges(&self) -> Result<Vec<Edge>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT id, source_id, target_id, target_name, target_file, relationship, confidence, original_name
             FROM edges WHERE target_id IS NULL",
        )?;
        let rows = stmt.query_map([], row_to_edge)?;
        collect_rows(rows)
    }

    pub fn get_resolved_edges(&self) -> Result<Vec<Edge>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT id, source_id, target_id, target_name, target_file, relationship, confidence, original_name
             FROM edges WHERE target_id IS NOT NULL",
        )?;
        let rows = stmt.query_map([], row_to_edge)?;
        collect_rows(rows)
    }

    pub fn resolve_edge(&self, edge_id: i64, target_id: i64, confidence: f64) -> Result<()> {
        let conn = self.writer.lock();
        conn.execute(
            "UPDATE edges SET target_id = ?, confidence = ? WHERE id = ?",
            params![target_id, confidence, edge_id],
        )?;
        Ok(())
    }

    pub fn remove_edges_for_source(&self, symbol_id: i64) -> Result<()> {
        let conn = self.writer.lock();
        conn.execute("DELETE FROM edges WHERE source_id = ?", [symbol_id])?;
        Ok(())
    }

    pub fn get_file_hash(&self, path: &str) -> Result<Option<String>> {
        let conn = self.reader()?;
        conn.query_row(
            "SELECT content_hash FROM index_meta WHERE file_path = ?",
            [path],
            |row| row.get(0),
        )
        .optional()
        .map_err(LoomError::from)
    }

    pub fn file_index_is_fresh(
        &self,
        path: &str,
        content_hash: &str,
        embedding_fingerprint: &str,
    ) -> Result<bool> {
        if self.get_file_hash(path)?.as_deref() != Some(content_hash) {
            return Ok(false);
        }
        if self.get_embedding_fingerprint(path)?.as_deref() != Some(embedding_fingerprint) {
            return Ok(false);
        }
        let conn = self.reader()?;
        let symbol_ids = select_symbol_ids_for_file(&conn, path)?;
        if symbol_ids.is_empty() {
            return Ok(true);
        }
        let vector_count = self
            .vector_store
            .count_embeddings_for_symbols(&conn, &symbol_ids)?;
        Ok(vector_count == i64::try_from(symbol_ids.len()).unwrap_or(i64::MAX))
    }

    pub fn set_file_hash(&self, path: &str, content_hash: &str) -> Result<()> {
        let conn = self.writer.lock();
        conn.execute(
            "INSERT OR REPLACE INTO index_meta
             (file_path, content_hash, embedding_fingerprint)
             VALUES (?, ?, ?)",
            params![path, content_hash, self.config.embedding_fingerprint()],
        )?;
        Ok(())
    }

    fn get_embedding_fingerprint(&self, path: &str) -> Result<Option<String>> {
        let conn = self.reader()?;
        conn.query_row(
            "SELECT embedding_fingerprint FROM index_meta WHERE file_path = ?",
            [path],
            |row| row.get(0),
        )
        .optional()
        .map_err(LoomError::from)
    }

    pub fn list_symbol_files(&self) -> Result<Vec<String>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare("SELECT DISTINCT file FROM symbols ORDER BY file")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        collect_rows(rows)
    }

    pub fn list_indexed_files(&self) -> Result<Vec<String>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare("SELECT file_path FROM index_meta ORDER BY file_path")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        collect_rows(rows)
    }

    pub fn get_import_edges_with_source_file(&self) -> Result<Vec<ImportEdgeRow>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT e.target_name, s.file, e.target_file, e.original_name
             FROM edges e
             JOIN symbols s ON s.id = e.source_id
             WHERE e.relationship = 'imports' AND e.target_file IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ImportEdgeRow {
                local_name: row.get(0)?,
                source_file: row.get(1)?,
                target_file: row.get(2)?,
                original_name: row.get(3)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn find_symbols_like_name(
        &self,
        pattern: &str,
        file: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Symbol>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = checked_sql_limit(limit)?;
        let conn = self.reader()?;
        if let Some(file) = file {
            let mut stmt = conn.prepare(
                "SELECT id, name, kind, file, line, end_line, language, context
                 FROM symbols WHERE name LIKE ? AND file = ? LIMIT ?",
            )?;
            let rows = stmt.query_map(params![pattern, file, limit], row_to_symbol)?;
            collect_rows(rows)
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, name, kind, file, line, end_line, language, context
                 FROM symbols WHERE name LIKE ? LIMIT ?",
            )?;
            let rows = stmt.query_map(params![pattern, limit], row_to_symbol)?;
            collect_rows(rows)
        }
    }

    pub fn resolve_edges_batch(&self, resolutions: &[(i64, i64, f64)]) -> Result<()> {
        let mut conn = self.writer.lock();
        let tx = conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("UPDATE edges SET target_id = ?, confidence = ? WHERE id = ?")?;
            for (edge_id, target_id, confidence) in resolutions {
                stmt.execute(params![target_id, confidence, edge_id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn resolve_signal_enclosures(&self) -> Result<usize> {
        let conn = self.writer.lock();
        let mut changed = 0usize;
        for table in ["behavior_facts", "callsites", "aliases"] {
            changed += conn.execute(
                &format!(
                    "UPDATE {table}
                     SET enclosing_symbol_id = (
                        SELECT s.id FROM symbols s
                        WHERE s.file = {table}.file
                          AND s.name = {table}.enclosing_symbol_name
                        ORDER BY s.line, s.id
                        LIMIT 1
                     )
                     WHERE enclosing_symbol_id IS NULL
                       AND enclosing_symbol_name IS NOT NULL
                       AND EXISTS (
                        SELECT 1 FROM symbols s
                        WHERE s.file = {table}.file
                          AND s.name = {table}.enclosing_symbol_name
                     )"
                ),
                [],
            )?;
        }
        Ok(changed)
    }

    pub fn resolve_callsites_from_edges(&self) -> Result<usize> {
        let conn = self.writer.lock();
        let changed = conn.execute(
            "UPDATE callsites
             SET resolved_target_id = (
                SELECT e.target_id FROM edges e
                WHERE e.source_id = callsites.enclosing_symbol_id
                  AND e.relationship IN ('calls', 'instantiates')
                  AND e.target_id IS NOT NULL
                  AND (e.target_name = callsites.unresolved_target
                       OR e.target_name = callsites.callee
                       OR e.target_name LIKE '%' || callsites.callee)
                ORDER BY e.confidence DESC, e.id
                LIMIT 1
             )
             WHERE resolved_target_id IS NULL
               AND enclosing_symbol_id IS NOT NULL
               AND EXISTS (
                SELECT 1 FROM edges e
                WHERE e.source_id = callsites.enclosing_symbol_id
                  AND e.relationship IN ('calls', 'instantiates')
                  AND e.target_id IS NOT NULL
                  AND (e.target_name = callsites.unresolved_target
                       OR e.target_name = callsites.callee
                       OR e.target_name LIKE '%' || callsites.callee)
             )",
            [],
        )?;
        Ok(changed)
    }

    pub fn refresh_role_cards(&self) -> Result<()> {
        let mut conn = self.writer.lock();
        let tx = conn.transaction()?;
        let files = {
            let mut stmt = tx.prepare("SELECT file FROM file_role_cards ORDER BY file")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            collect_rows(rows)?
        };
        let total_symbols = tx.query_row("SELECT COUNT(*) FROM symbols", [], |row| {
            row.get::<_, i64>(0)
        })?;
        for file in files {
            let incoming = tx.query_row(
                "SELECT COUNT(*)
                 FROM edges e
                 JOIN symbols target ON target.id = e.target_id
                 JOIN symbols source ON source.id = e.source_id
                 WHERE target.file = ? AND source.file != target.file",
                [&file],
                |row| row.get::<_, i64>(0),
            )?;
            let denominator = total_symbols.saturating_sub(1).max(1) as f64;
            let centrality = (incoming as f64 / denominator).clamp(0.0, 1.0);
            let related = related_files_for_card(&tx, &file)?;
            let tests = tests_touching_file(&tx, &file)?;
            tx.execute(
                "UPDATE file_role_cards
                 SET centrality = ?, top_related_files = ?, tests_touching = ?,
                     updated_at = datetime('now')
                 WHERE file = ?",
                params![
                    centrality,
                    json_string(&related)?,
                    json_string(&tests)?,
                    file
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_cochange(&self, file_a: &str, file_b: &str, frequency: i64) -> Result<()> {
        self.upsert_cochange_with_recency(file_a, file_b, frequency, 0.0)
    }

    pub fn upsert_cochange_with_recency(
        &self,
        file_a: &str,
        file_b: &str,
        frequency: i64,
        recency: f64,
    ) -> Result<()> {
        let (a, b) = canonical_pair(file_a, file_b);
        let conn = self.writer.lock();
        conn.execute(
            "INSERT INTO cochange (file_a, file_b, frequency, recency) VALUES (?, ?, ?, ?)
             ON CONFLICT(file_a, file_b) DO UPDATE SET
                frequency = excluded.frequency,
                recency = excluded.recency",
            params![a, b, frequency, recency],
        )?;
        Ok(())
    }

    pub fn replace_cochanges(&self, cochanges: &[(String, String, i64, f64)]) -> Result<()> {
        let mut conn = self.writer.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM cochange", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO cochange (file_a, file_b, frequency, recency) VALUES (?, ?, ?, ?)",
            )?;
            for (file_a, file_b, frequency, recency) in cochanges {
                let (a, b) = canonical_pair(file_a, file_b);
                stmt.execute(params![a, b, frequency, recency])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_cochange(&self, file_a: &str, file_b: &str) -> Result<Option<CochangeRow>> {
        let (a, b) = canonical_pair(file_a, file_b);
        let conn = self.reader()?;
        conn.query_row(
            "SELECT file_a, file_b, frequency, recency FROM cochange
             WHERE file_a = ? AND file_b = ?",
            params![a, b],
            |row| {
                Ok(CochangeRow {
                    file_a: row.get(0)?,
                    file_b: row.get(1)?,
                    frequency: row.get(2)?,
                    recency: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(LoomError::from)
    }

    pub fn get_cochange_frequency(&self, file_a: &str, file_b: &str) -> Result<i64> {
        let (a, b) = canonical_pair(file_a, file_b);
        let conn = self.reader()?;
        let frequency = conn
            .query_row(
                "SELECT frequency FROM cochange WHERE file_a = ? AND file_b = ?",
                params![a, b],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(frequency)
    }

    pub fn get_top_cochanges(&self, file: &str, limit: usize) -> Result<Vec<(String, i64)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = checked_sql_limit(limit)?;
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT CASE WHEN file_a = ? THEN file_b ELSE file_a END AS other_file, frequency
             FROM cochange WHERE file_a = ? OR file_b = ?
             ORDER BY frequency DESC, recency DESC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![file, file, file, limit], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        collect_rows(rows)
    }

    pub fn get_stats(&self) -> Result<StoreStats> {
        let conn = self.reader()?;
        let symbols = count_table(&conn, "symbols")?;
        let edges = count_table(&conn, "edges")?;
        let files = count_table(&conn, "index_meta")?;
        let vectors = self.vector_store.count(&conn)?;
        let last_indexed =
            conn.query_row("SELECT MAX(last_indexed) FROM index_meta", [], |row| {
                row.get(0)
            })?;
        let stale_files = conn.query_row(
            "SELECT COUNT(*) FROM index_meta WHERE last_indexed < datetime('now', '-1 hour')",
            [],
            |row| row.get(0),
        )?;
        let cochange_pairs = count_table(&conn, "cochange")?;
        let behavior_facts = count_table(&conn, "behavior_facts")?;
        let callsites = count_table(&conn, "callsites")?;
        let aliases = count_table(&conn, "aliases")?;
        let role_cards = count_table(&conn, "file_role_cards")?;
        Ok(StoreStats {
            symbols,
            edges,
            files,
            vectors,
            behavior_facts,
            callsites,
            aliases,
            role_cards,
            last_indexed,
            stale_files,
            cochange_pairs,
        })
    }

    pub fn index_revision(&self) -> Result<String> {
        let conn = self.reader()?;
        let mut hasher = Sha256::new();
        hash_query_rows(
            &conn,
            "SELECT id, name, kind, file, line, end_line, language, context
             FROM symbols ORDER BY id",
            8,
            &mut hasher,
        )?;
        hash_query_rows(
            &conn,
            "SELECT source_id, COALESCE(target_id, -1), target_name,
                    COALESCE(target_file, ''), relationship, confidence,
                    COALESCE(original_name, '')
             FROM edges ORDER BY id",
            7,
            &mut hasher,
        )?;
        hash_query_rows(
            &conn,
            "SELECT file_path, content_hash, embedding_fingerprint
             FROM index_meta ORDER BY file_path",
            3,
            &mut hasher,
        )?;
        hash_query_rows(
            &conn,
            "SELECT fact_type, value, file, line, end_line,
                    COALESCE(enclosing_symbol_id, -1), COALESCE(enclosing_symbol_name, ''),
                    occurrence_count
             FROM behavior_facts ORDER BY file, line, fact_type, value, id",
            8,
            &mut hasher,
        )?;
        hash_query_rows(
            &conn,
            "SELECT file, line, end_line, callee, COALESCE(receiver, ''),
                    unresolved_target, COALESCE(resolved_target_id, -1),
                    argument_summaries, imported_aliases,
                    COALESCE(enclosing_symbol_id, -1), COALESCE(enclosing_symbol_name, ''),
                    confidence, generic, downweighted
             FROM callsites ORDER BY file, line, unresolved_target, id",
            14,
            &mut hasher,
        )?;
        hash_query_rows(
            &conn,
            "SELECT file, line, end_line, local_name, imported_name, source,
                    alias_kind, COALESCE(enclosing_symbol_id, -1),
                    COALESCE(enclosing_symbol_name, '')
             FROM aliases ORDER BY file, line, local_name, source, id",
            9,
            &mut hasher,
        )?;
        hash_query_rows(
            &conn,
            "SELECT file, content_hash, primary_responsibility, exported_symbols,
                    imported_dependencies, behavior_facts, centrality, tests_touching,
                    top_related_files
             FROM file_role_cards ORDER BY file",
            9,
            &mut hasher,
        )?;
        let digest = hasher.finalize();
        let mut revision = String::from("idx-");
        for byte in digest.iter().take(12) {
            write!(&mut revision, "{byte:02x}").expect("writing to a String should not fail");
        }
        Ok(revision)
    }

    pub fn reader_pragma_value(&self, pragma: ReaderPragma) -> Result<String> {
        let conn = self.reader()?;
        conn.query_row(pragma.sql(), [], |row| row.get::<_, String>(0))
            .or_else(|_| {
                conn.query_row(pragma.sql(), [], |row| {
                    let value: i64 = row.get(0)?;
                    Ok(value.to_string())
                })
            })
            .map_err(LoomError::from)
    }

    fn reader(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        let conn = self.readers.get()?;
        apply_pragmas(&conn)?;
        Ok(conn)
    }

    fn method_suffix_lookup(&self, name: &str, file: Option<&str>) -> Result<Vec<Symbol>> {
        let pattern = format!("%.{name}");
        let conn = self.reader()?;
        if let Some(file) = file {
            let file_like = if file.contains('/') {
                file.to_string()
            } else {
                format!("%/{file}")
            };
            let mut stmt = conn.prepare(
                "SELECT id, name, kind, file, line, end_line, language, context
                 FROM symbols WHERE name LIKE ? AND (file = ? OR file LIKE ?)",
            )?;
            let rows = stmt.query_map(params![pattern, file, file_like], row_to_symbol)?;
            collect_rows(rows)
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, name, kind, file, line, end_line, language, context
                 FROM symbols WHERE name LIKE ? LIMIT 20",
            )?;
            let rows = stmt.query_map([pattern], row_to_symbol)?;
            collect_rows(rows)
        }
    }

    fn query_edges(&self, sql: &str, symbol_id: i64) -> Result<Vec<Edge>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([symbol_id], row_to_edge)?;
        collect_rows(rows)
    }
}

pub fn sanitize_fts_query(query: &str) -> String {
    tokenize_fts_query(query)
        .into_iter()
        .filter(|term| {
            term.text
                .chars()
                .any(|character| character.is_alphanumeric() || character == '_')
        })
        .map(|term| sanitize_fts_term(&term))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FtsQueryTerm {
    text: String,
    quoted: bool,
}

fn tokenize_fts_query(query: &str) -> Vec<FtsQueryTerm> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut current_quoted = false;

    for character in query.chars() {
        match character {
            '"' if in_quote => {
                push_fts_term(&mut terms, &mut current, true);
                in_quote = false;
                current_quoted = false;
            }
            '"' if current.trim().is_empty() => {
                push_fts_term(&mut terms, &mut current, current_quoted);
                in_quote = true;
                current_quoted = true;
            }
            character if character.is_whitespace() && !in_quote => {
                push_fts_term(&mut terms, &mut current, current_quoted);
                current_quoted = false;
            }
            _ => current.push(character),
        }
    }
    push_fts_term(&mut terms, &mut current, current_quoted);
    terms
}

fn push_fts_term(terms: &mut Vec<FtsQueryTerm>, current: &mut String, quoted: bool) {
    let text = current.trim();
    if !text.is_empty() {
        terms.push(FtsQueryTerm {
            text: text.to_string(),
            quoted,
        });
    }
    current.clear();
}

fn sanitize_fts_term(term: &FtsQueryTerm) -> String {
    let upper = term.text.to_ascii_uppercase();
    let needs_quotes = term.quoted
        || FTS5_SPECIAL.contains(&upper.as_str())
        || term
            .text
            .chars()
            .any(|character| !character.is_alphanumeric() && character != '_');
    if needs_quotes {
        format!("\"{}\"", term.text.replace('"', "\"\""))
    } else {
        term.text.clone()
    }
}

fn classify_fts_match(query: &str) -> String {
    if tokenize_fts_query(query)
        .into_iter()
        .any(|term| term.quoted && term.text.chars().any(char::is_whitespace))
    {
        "exact_phrase".to_string()
    } else {
        "token_match".to_string()
    }
}

fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn row_to_symbol(row: &Row<'_>) -> rusqlite::Result<Symbol> {
    Ok(Symbol {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        file: row.get(3)?,
        line: row.get(4)?,
        end_line: row.get(5)?,
        language: row.get(6)?,
        context: row.get(7)?,
    })
}

fn row_to_behavior_fact(row: &Row<'_>) -> rusqlite::Result<BehaviorFact> {
    Ok(BehaviorFact {
        id: row.get(0)?,
        fact_type: row.get(1)?,
        value: row.get(2)?,
        file: row.get(3)?,
        line: row.get(4)?,
        end_line: row.get(5)?,
        enclosing_symbol_id: row.get(6)?,
        enclosing_symbol_name: row.get(7)?,
        occurrence_count: row.get(8)?,
    })
}

fn row_to_behavior_fact_hit(
    row: &Row<'_>,
    sanitized_query: &str,
    match_kind: &str,
) -> rusqlite::Result<BehaviorFactHit> {
    let fact = row_to_behavior_fact(row)?;
    let rank = row.get(9)?;
    let snippets = [
        ("fact_type", row.get::<_, String>(10)?),
        ("value", row.get::<_, String>(11)?),
        ("file", row.get::<_, String>(12)?),
    ];
    let (field, marked_snippet) = snippets
        .iter()
        .find(|(_, snippet)| snippet.contains(FTS_MATCH_START))
        .unwrap_or(&snippets[1]);
    let matched_text = extract_marked_text(marked_snippet)
        .unwrap_or_else(|| fallback_matched_text(sanitized_query));
    Ok(BehaviorFactHit {
        fact,
        lexical_evidence: LexicalEvidence {
            snippet: bounded_text(&strip_match_markers(marked_snippet), MAX_FTS_SNIPPET_CHARS),
            matched_text: bounded_text(&matched_text, MAX_FTS_SNIPPET_CHARS),
            rank,
            field: (*field).to_string(),
            reason: format!("fact_fts:{field}"),
            match_kind: match_kind.to_string(),
            sanitized_query: sanitized_query.to_string(),
        },
    })
}

fn row_to_callsite(row: &Row<'_>) -> rusqlite::Result<Callsite> {
    let argument_summaries = json_vec(row.get::<_, String>(8)?);
    let imported_aliases = json_vec(row.get::<_, String>(9)?);
    Ok(Callsite {
        id: row.get(0)?,
        file: row.get(1)?,
        line: row.get(2)?,
        end_line: row.get(3)?,
        callee: row.get(4)?,
        receiver: row.get(5)?,
        unresolved_target: row.get(6)?,
        resolved_target_id: row.get(7)?,
        argument_summaries,
        imported_aliases,
        enclosing_symbol_id: row.get(10)?,
        enclosing_symbol_name: row.get(11)?,
        confidence: row.get(12)?,
        generic: row.get::<_, i64>(13)? != 0,
        downweighted: row.get::<_, i64>(14)? != 0,
    })
}

fn row_to_alias(row: &Row<'_>) -> rusqlite::Result<AliasRecord> {
    Ok(AliasRecord {
        id: row.get(0)?,
        file: row.get(1)?,
        line: row.get(2)?,
        end_line: row.get(3)?,
        local_name: row.get(4)?,
        imported_name: row.get(5)?,
        source: row.get(6)?,
        alias_kind: row.get(7)?,
        enclosing_symbol_id: row.get(8)?,
        enclosing_symbol_name: row.get(9)?,
    })
}

fn row_to_role_card(row: &Row<'_>) -> rusqlite::Result<FileRoleCard> {
    Ok(FileRoleCard {
        file: row.get(0)?,
        content_hash: row.get(1)?,
        primary_responsibility: row.get(2)?,
        exported_symbols: json_vec(row.get::<_, String>(3)?),
        imported_dependencies: json_vec(row.get::<_, String>(4)?),
        behavior_facts: json_vec(row.get::<_, String>(5)?),
        centrality: row.get(6)?,
        tests_touching: json_vec(row.get::<_, String>(7)?),
        top_related_files: json_vec(row.get::<_, String>(8)?),
    })
}

fn row_to_fts_result(
    row: &Row<'_>,
    sanitized_query: &str,
    match_kind: &str,
) -> rusqlite::Result<FtsSearchResult> {
    let symbol = row_to_symbol(row)?;
    let rank = row.get(8)?;
    let snippets = [
        ("name", row.get::<_, String>(9)?),
        ("kind", row.get::<_, String>(10)?),
        ("file", row.get::<_, String>(11)?),
        ("context", row.get::<_, String>(12)?),
    ];
    let (field, marked_snippet) = snippets
        .iter()
        .find(|(_, snippet)| snippet.contains(FTS_MATCH_START))
        .unwrap_or(&snippets[3]);
    let matched_text = extract_marked_text(marked_snippet)
        .unwrap_or_else(|| fallback_matched_text(sanitized_query));
    let snippet = bounded_text(&strip_match_markers(marked_snippet), MAX_FTS_SNIPPET_CHARS);

    Ok(FtsSearchResult {
        symbol,
        evidence: LexicalEvidence {
            snippet,
            matched_text: bounded_text(&matched_text, MAX_FTS_SNIPPET_CHARS),
            rank,
            field: (*field).to_string(),
            reason: format!("fts:{field}"),
            match_kind: match_kind.to_string(),
            sanitized_query: sanitized_query.to_string(),
        },
    })
}

fn row_to_edge(row: &Row<'_>) -> rusqlite::Result<Edge> {
    Ok(Edge {
        id: row.get(0)?,
        source_id: row.get(1)?,
        target_id: row.get(2)?,
        target_name: row.get(3)?,
        target_file: row.get(4)?,
        relationship: row.get(5)?,
        confidence: row.get(6)?,
        original_name: row.get(7)?,
    })
}

fn insert_behavior_facts_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    facts: &[BehaviorFact],
) -> Result<()> {
    let mut fact_stmt = tx.prepare(
        "INSERT INTO behavior_facts
         (fact_type, value, file, line, end_line, enclosing_symbol_id,
          enclosing_symbol_name, occurrence_count)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    let mut fts_stmt = tx.prepare(
        "INSERT INTO behavior_facts_fts (rowid, fact_type, value, file)
         VALUES (?, ?, ?, ?)",
    )?;
    for fact in facts {
        fact_stmt.execute(params![
            fact.fact_type,
            fact.value,
            fact.file,
            fact.line,
            fact.end_line,
            fact.enclosing_symbol_id,
            fact.enclosing_symbol_name,
            fact.occurrence_count
        ])?;
        let fact_id = tx.last_insert_rowid();
        fts_stmt.execute(params![fact_id, fact.fact_type, fact.value, fact.file])?;
    }
    Ok(())
}

fn insert_callsites_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    callsites: &[Callsite],
) -> Result<()> {
    let mut stmt = tx.prepare(
        "INSERT INTO callsites
         (file, line, end_line, callee, receiver, unresolved_target, resolved_target_id,
          argument_summaries, imported_aliases, enclosing_symbol_id, enclosing_symbol_name,
          confidence, generic, downweighted)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    for callsite in callsites {
        stmt.execute(params![
            callsite.file,
            callsite.line,
            callsite.end_line,
            callsite.callee,
            callsite.receiver,
            callsite.unresolved_target,
            callsite.resolved_target_id,
            json_string(&callsite.argument_summaries)?,
            json_string(&callsite.imported_aliases)?,
            callsite.enclosing_symbol_id,
            callsite.enclosing_symbol_name,
            callsite.confidence,
            bool_int(callsite.generic),
            bool_int(callsite.downweighted)
        ])?;
    }
    Ok(())
}

fn insert_aliases_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    aliases: &[AliasRecord],
) -> Result<()> {
    let mut stmt = tx.prepare(
        "INSERT INTO aliases
         (file, line, end_line, local_name, imported_name, source, alias_kind,
          enclosing_symbol_id, enclosing_symbol_name)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    for alias in aliases {
        stmt.execute(params![
            alias.file,
            alias.line,
            alias.end_line,
            alias.local_name,
            alias.imported_name,
            alias.source,
            alias.alias_kind,
            alias.enclosing_symbol_id,
            alias.enclosing_symbol_name
        ])?;
    }
    Ok(())
}

fn upsert_role_card_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    role_card: &FileRoleCard,
) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO file_role_cards
         (file, content_hash, primary_responsibility, exported_symbols,
          imported_dependencies, behavior_facts, centrality, tests_touching,
          top_related_files, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))",
        params![
            role_card.file,
            role_card.content_hash,
            role_card.primary_responsibility,
            json_string(&role_card.exported_symbols)?,
            json_string(&role_card.imported_dependencies)?,
            json_string(&role_card.behavior_facts)?,
            role_card.centrality,
            json_string(&role_card.tests_touching)?,
            json_string(&role_card.top_related_files)?
        ],
    )?;
    Ok(())
}

fn bool_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn json_string<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|source| {
        LoomError::InvalidInput(format!("failed to encode indexed signal JSON: {source}"))
    })
}

fn json_vec(value: String) -> Vec<String> {
    serde_json::from_str(&value).unwrap_or_default()
}

fn extract_marked_text(snippet: &str) -> Option<String> {
    let start = snippet.find(FTS_MATCH_START)? + FTS_MATCH_START.len();
    let rest = &snippet[start..];
    let end = rest.find(FTS_MATCH_END)?;
    Some(rest[..end].to_string())
}

fn fallback_matched_text(sanitized_query: &str) -> String {
    sanitized_query
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches('"')
        .replace("\"\"", "\"")
}

fn strip_match_markers(snippet: &str) -> String {
    snippet
        .replace(FTS_MATCH_START, "")
        .replace(FTS_MATCH_END, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}

fn collect_rows<T, F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<T>>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}

fn hash_query_rows(
    conn: &Connection,
    sql: &str,
    column_count: usize,
    hasher: &mut Sha256,
) -> Result<()> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        for index in 0..column_count {
            let value: rusqlite::types::Value = row.get(index)?;
            hasher.update(format!("{value:?}").as_bytes());
            hasher.update([0]);
        }
        hasher.update(b"\n");
    }
    Ok(())
}

fn select_symbol_ids_for_file(conn: &Connection, path: &str) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM symbols WHERE file = ?")?;
    let rows = stmt.query_map([path], |row| row.get(0))?;
    collect_rows(rows)
}

fn select_behavior_fact_ids_for_file(conn: &Connection, path: &str) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM behavior_facts WHERE file = ?")?;
    let rows = stmt.query_map([path], |row| row.get(0))?;
    collect_rows(rows)
}

fn related_files_for_card(conn: &Connection, file: &str) -> Result<Vec<String>> {
    let mut scores = BTreeMap::<String, i64>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT other_file, SUM(weight) AS score FROM (
                SELECT target.file AS other_file, 2 AS weight
                FROM edges e
                JOIN symbols source ON source.id = e.source_id
                JOIN symbols target ON target.id = e.target_id
                WHERE source.file = ? AND target.file != source.file
                UNION ALL
                SELECT source.file AS other_file, 1 AS weight
                FROM edges e
                JOIN symbols source ON source.id = e.source_id
                JOIN symbols target ON target.id = e.target_id
                WHERE target.file = ? AND source.file != target.file
            )
             GROUP BY other_file ORDER BY score DESC, other_file LIMIT 8",
        )?;
        let rows = stmt.query_map(params![file, file], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (other, score) = row?;
            scores.insert(other, score);
        }
    }
    for (other, frequency) in top_cochanges_with_conn(conn, file, 8)? {
        *scores.entry(other).or_insert(0) += frequency;
    }
    let mut related = scores.into_iter().collect::<Vec<_>>();
    related.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    related.truncate(5);
    Ok(related.into_iter().map(|(file, _)| file).collect())
}

fn tests_touching_file(conn: &Connection, file: &str) -> Result<Vec<String>> {
    let stem = std::path::Path::new(file)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if stem.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = format!("%{stem}%");
    let mut stmt = conn.prepare(
        "SELECT file_path FROM index_meta
         WHERE file_path != ?
           AND (file_path LIKE '%test%' OR file_path LIKE '%spec%')
           AND file_path LIKE ?
         ORDER BY file_path LIMIT 8",
    )?;
    let rows = stmt.query_map(params![file, pattern], |row| row.get(0))?;
    collect_rows(rows)
}

fn top_cochanges_with_conn(
    conn: &Connection,
    file: &str,
    limit: usize,
) -> Result<Vec<(String, i64)>> {
    let limit = checked_sql_limit(limit)?;
    let mut stmt = conn.prepare(
        "SELECT CASE WHEN file_a = ? THEN file_b ELSE file_a END AS other_file, frequency
         FROM cochange WHERE file_a = ? OR file_b = ?
         ORDER BY frequency DESC, recency DESC LIMIT ?",
    )?;
    let rows = stmt.query_map(params![file, file, file, limit], |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    collect_rows(rows)
}

fn remove_file_in_transaction(
    conn: &Connection,
    vector_store: &dyn VectorStore,
    path: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE edges SET target_id = NULL, confidence = 0.0
         WHERE target_id IN (SELECT id FROM symbols WHERE file = ?)",
        [path],
    )?;
    let symbol_ids = select_symbol_ids_for_file(conn, path)?;
    if !symbol_ids.is_empty() {
        vector_store.delete_embeddings(conn, &symbol_ids)?;
        let placeholders = repeat_placeholders(symbol_ids.len());
        let sql = format!("DELETE FROM symbols_fts WHERE rowid IN ({placeholders})");
        conn.execute(&sql, params_from_iter(symbol_ids.iter()))?;
    }
    let fact_ids = select_behavior_fact_ids_for_file(conn, path)?;
    if !fact_ids.is_empty() {
        let placeholders = repeat_placeholders(fact_ids.len());
        let sql = format!("DELETE FROM behavior_facts_fts WHERE rowid IN ({placeholders})");
        conn.execute(&sql, params_from_iter(fact_ids.iter()))?;
    }
    conn.execute("DELETE FROM behavior_facts WHERE file = ?", [path])?;
    conn.execute("DELETE FROM callsites WHERE file = ?", [path])?;
    conn.execute("DELETE FROM aliases WHERE file = ?", [path])?;
    conn.execute("DELETE FROM file_role_cards WHERE file = ?", [path])?;
    conn.execute("DELETE FROM symbols WHERE file = ?", [path])?;
    conn.execute("DELETE FROM index_meta WHERE file_path = ?", [path])?;
    conn.execute(
        "DELETE FROM cochange WHERE file_a = ? OR file_b = ?",
        params![path, path],
    )?;
    Ok(())
}

fn filter_file_suffix(symbols: Vec<Symbol>, file: &str) -> Vec<Symbol> {
    let slash_suffix = format!("/{file}");
    symbols
        .into_iter()
        .filter(|symbol| symbol.file.ends_with(file) || symbol.file.ends_with(&slash_suffix))
        .collect()
}

fn canonical_pair(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_string(), right.to_string())
    } else {
        (right.to_string(), left.to_string())
    }
}

fn count_table(conn: &Connection, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(LoomError::from)
}

fn checked_sql_limit(limit: usize) -> Result<i64> {
    if limit > MAX_SQL_LIMIT {
        return Err(LoomError::InvalidInput(format!(
            "limit must be <= {MAX_SQL_LIMIT}, got {limit}"
        )));
    }
    i64::try_from(limit).map_err(|_| LoomError::InvalidInput("limit is too large".to_string()))
}
