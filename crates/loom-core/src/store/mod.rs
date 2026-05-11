pub mod migrations;
pub mod vector;

use crate::config::{LoomConfig, VectorBackendConfig};
use crate::error::{LoomError, Result};
use crate::models::{Edge, StoreStats, Symbol};
use migrations::run_migrations;
use parking_lot::Mutex;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;
use vector::{
    register_sqlite_vec_once, repeat_placeholders, BlobVectorStore, SqliteVecStore, VectorStore,
};

const FTS5_SPECIAL: [&str; 4] = ["AND", "OR", "NOT", "NEAR"];
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
        path: &str,
        content_hash: &str,
        symbols: &[Symbol],
        embeddings: &[Vec<f32>],
        build_edges: F,
    ) -> Result<(usize, usize)>
    where
        F: FnOnce(&[i64]) -> Result<Vec<Edge>>,
    {
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

        for (symbol_id, embedding) in symbol_ids.iter().zip(embeddings.iter()) {
            self.vector_store.insert_embedding(
                &tx,
                *symbol_id,
                embedding,
                self.config.embedding_dimensions,
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO index_meta (file_path, content_hash) VALUES (?, ?)",
            params![path, content_hash],
        )?;
        tx.commit()?;
        Ok((symbols.len(), edges.len()))
    }

    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<Symbol>> {
        let sanitized = sanitize_fts_query(query);
        if sanitized.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let limit = checked_sql_limit(limit)?;
        let conn = self.reader()?;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.kind, s.file, s.line, s.end_line, s.language, s.context
             FROM symbols_fts fts
             JOIN symbols s ON s.id = fts.rowid
             WHERE symbols_fts MATCH ?
             ORDER BY rank LIMIT ?",
        )?;
        let rows = stmt.query_map(params![sanitized, limit], row_to_symbol)?;
        collect_rows(rows)
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

    pub fn file_index_is_fresh(&self, path: &str, content_hash: &str) -> Result<bool> {
        if self.get_file_hash(path)?.as_deref() != Some(content_hash) {
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
            "INSERT OR REPLACE INTO index_meta (file_path, content_hash) VALUES (?, ?)",
            params![path, content_hash],
        )?;
        Ok(())
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
        Ok(StoreStats {
            symbols,
            edges,
            files,
            vectors,
            last_indexed,
            stale_files,
            cochange_pairs,
        })
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
    let stripped = query.trim();
    if stripped.is_empty() {
        return String::new();
    }
    stripped
        .split_whitespace()
        .map(|token| {
            let upper = token.to_ascii_uppercase();
            if FTS5_SPECIAL.contains(&upper.as_str())
                || token
                    .chars()
                    .any(|character| ['-', '*', '"', '^', ':', '.'].contains(&character))
            {
                format!("\"{}\"", token.replace('"', "\"\""))
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

fn select_symbol_ids_for_file(conn: &Connection, path: &str) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM symbols WHERE file = ?")?;
    let rows = stmt.query_map([path], |row| row.get(0))?;
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
