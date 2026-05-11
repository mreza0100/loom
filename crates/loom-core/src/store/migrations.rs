use crate::{error::Result, store::vector::VectorStore};
use rusqlite::Connection;

pub const CURRENT_SCHEMA_VERSION: i64 = 5;

pub fn run_migrations(
    conn: &Connection,
    vector_store: &dyn VectorStore,
    dimensions: usize,
) -> Result<()> {
    let version = schema_version(conn)?;
    if version < 1 {
        create_base_schema(conn)?;
        set_schema_version(conn, 1)?;
    }
    if schema_version(conn)? < 2 {
        ensure_cochange_recency(conn)?;
        set_schema_version(conn, 2)?;
    }
    if schema_version(conn)? < 3 {
        vector_store.create_schema(conn, dimensions)?;
        set_schema_version(conn, 3)?;
    }
    if schema_version(conn)? < 4 {
        ensure_index_meta_embedding_fingerprint(conn)?;
        set_schema_version(conn, 4)?;
    }
    if schema_version(conn)? < 5 {
        create_signal_schema(conn)?;
        set_schema_version(conn, 5)?;
    }

    // Older Rust databases can have user_version = 0 while already carrying
    // current tables. Keep the guards idempotent so opening them upgrades cleanly.
    create_base_schema(conn)?;
    ensure_cochange_recency(conn)?;
    ensure_index_meta_embedding_fingerprint(conn)?;
    create_signal_schema(conn)?;
    vector_store.create_schema(conn, dimensions)?;
    set_schema_version(conn, CURRENT_SCHEMA_VERSION)?;
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(Into::into)
}

fn set_schema_version(conn: &Connection, version: i64) -> Result<()> {
    conn.pragma_update(None, "user_version", version)?;
    Ok(())
}

fn create_base_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS symbols (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            file TEXT NOT NULL,
            line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            language TEXT NOT NULL,
            context TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file);

        CREATE TABLE IF NOT EXISTS edges (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
            target_id INTEGER REFERENCES symbols(id) ON DELETE SET NULL,
            target_name TEXT NOT NULL,
            target_file TEXT,
            relationship TEXT NOT NULL,
            confidence REAL NOT NULL DEFAULT 0.0,
            original_name TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
        CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
        CREATE INDEX IF NOT EXISTS idx_edges_target_name ON edges(target_name);
        CREATE INDEX IF NOT EXISTS idx_edges_unresolved ON edges(target_id) WHERE target_id IS NULL;

        CREATE TABLE IF NOT EXISTS index_meta (
            file_path TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            embedding_fingerprint TEXT NOT NULL DEFAULT '',
            last_indexed TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
            name, kind, file, context, content=symbols, content_rowid=id
        );

        CREATE TABLE IF NOT EXISTS cochange (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_a TEXT NOT NULL,
            file_b TEXT NOT NULL,
            frequency INTEGER NOT NULL DEFAULT 1,
            recency REAL NOT NULL DEFAULT 0.0,
            UNIQUE(file_a, file_b)
        );
        CREATE INDEX IF NOT EXISTS idx_cochange_a ON cochange(file_a);
        CREATE INDEX IF NOT EXISTS idx_cochange_b ON cochange(file_b);
        ",
    )?;
    Ok(())
}

fn create_signal_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS behavior_facts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            fact_type TEXT NOT NULL,
            value TEXT NOT NULL,
            file TEXT NOT NULL,
            line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            enclosing_symbol_id INTEGER REFERENCES symbols(id) ON DELETE SET NULL,
            enclosing_symbol_name TEXT,
            occurrence_count INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_behavior_facts_file ON behavior_facts(file);
        CREATE INDEX IF NOT EXISTS idx_behavior_facts_type_value ON behavior_facts(fact_type, value);
        CREATE INDEX IF NOT EXISTS idx_behavior_facts_enclosing_symbol
            ON behavior_facts(enclosing_symbol_id);
        CREATE VIRTUAL TABLE IF NOT EXISTS behavior_facts_fts USING fts5(
            fact_type, value, file, content=behavior_facts, content_rowid=id
        );

        CREATE TABLE IF NOT EXISTS aliases (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file TEXT NOT NULL,
            line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            local_name TEXT NOT NULL,
            imported_name TEXT NOT NULL,
            source TEXT NOT NULL,
            alias_kind TEXT NOT NULL,
            enclosing_symbol_id INTEGER REFERENCES symbols(id) ON DELETE SET NULL,
            enclosing_symbol_name TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_aliases_file ON aliases(file);
        CREATE INDEX IF NOT EXISTS idx_aliases_local ON aliases(file, local_name);

        CREATE TABLE IF NOT EXISTS callsites (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file TEXT NOT NULL,
            line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            callee TEXT NOT NULL,
            receiver TEXT,
            unresolved_target TEXT NOT NULL,
            resolved_target_id INTEGER REFERENCES symbols(id) ON DELETE SET NULL,
            argument_summaries TEXT NOT NULL DEFAULT '[]',
            imported_aliases TEXT NOT NULL DEFAULT '[]',
            enclosing_symbol_id INTEGER REFERENCES symbols(id) ON DELETE SET NULL,
            enclosing_symbol_name TEXT,
            confidence REAL NOT NULL DEFAULT 0.0,
            generic INTEGER NOT NULL DEFAULT 0,
            downweighted INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_callsites_file ON callsites(file);
        CREATE INDEX IF NOT EXISTS idx_callsites_callee ON callsites(callee);
        CREATE INDEX IF NOT EXISTS idx_callsites_enclosing_symbol
            ON callsites(enclosing_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_callsites_resolved_target
            ON callsites(resolved_target_id);

        CREATE TABLE IF NOT EXISTS file_role_cards (
            file TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            primary_responsibility TEXT NOT NULL,
            exported_symbols TEXT NOT NULL DEFAULT '[]',
            imported_dependencies TEXT NOT NULL DEFAULT '[]',
            behavior_facts TEXT NOT NULL DEFAULT '[]',
            centrality REAL NOT NULL DEFAULT 0.0,
            tests_touching TEXT NOT NULL DEFAULT '[]',
            top_related_files TEXT NOT NULL DEFAULT '[]',
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        ",
    )?;
    Ok(())
}

fn ensure_cochange_recency(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "cochange",
        "recency",
        "ALTER TABLE cochange ADD COLUMN recency REAL NOT NULL DEFAULT 0.0",
    )
}

fn ensure_index_meta_embedding_fingerprint(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "index_meta",
        "embedding_fingerprint",
        "ALTER TABLE index_meta ADD COLUMN embedding_fingerprint TEXT NOT NULL DEFAULT ''",
    )
}

fn ensure_column(conn: &Connection, table: &str, column: &str, alter_sql: &str) -> Result<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&pragma)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }
    if !columns.iter().any(|existing| existing == column) {
        conn.execute(alter_sql, [])?;
    }
    Ok(())
}
