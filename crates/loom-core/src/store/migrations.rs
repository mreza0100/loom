use crate::{error::Result, store::vector::VectorStore};
use rusqlite::Connection;

pub const CURRENT_SCHEMA_VERSION: i64 = 3;

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

    // Older Rust databases can have user_version = 0 while already carrying
    // current tables. Keep the guards idempotent so opening them upgrades cleanly.
    create_base_schema(conn)?;
    ensure_cochange_recency(conn)?;
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

fn ensure_cochange_recency(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(cochange)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }
    if !columns.iter().any(|column| column == "recency") {
        conn.execute(
            "ALTER TABLE cochange ADD COLUMN recency REAL NOT NULL DEFAULT 0.0",
            [],
        )?;
    }
    Ok(())
}
