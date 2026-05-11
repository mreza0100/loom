use crate::error::{LoomError, Result};
use rusqlite::{params, params_from_iter, Connection};
use std::os::raw::{c_char, c_int};
use std::sync::OnceLock;

pub trait VectorStore: Send + Sync {
    fn backend_name(&self) -> &'static str;
    fn create_schema(&self, conn: &Connection, dimensions: usize) -> Result<()>;
    fn insert_embedding(
        &self,
        conn: &Connection,
        symbol_id: i64,
        embedding: &[f32],
        dimensions: usize,
    ) -> Result<()>;
    fn delete_embeddings(&self, conn: &Connection, symbol_ids: &[i64]) -> Result<()>;
    fn clear(&self, conn: &Connection) -> Result<()>;
    fn count(&self, conn: &Connection) -> Result<i64>;
    fn search(
        &self,
        conn: &Connection,
        embedding: &[f32],
        dimensions: usize,
        limit: usize,
    ) -> Result<Vec<(i64, f64)>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorBackend {
    SqliteVec,
    Blob,
}

impl VectorBackend {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SqliteVec => "sqlite-vec",
            Self::Blob => "blob",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BlobVectorStore;

impl VectorStore for BlobVectorStore {
    fn backend_name(&self) -> &'static str {
        VectorBackend::Blob.as_str()
    }

    fn create_schema(&self, conn: &Connection, _dimensions: usize) -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS symbol_embeddings (
                symbol_id INTEGER PRIMARY KEY REFERENCES symbols(id) ON DELETE CASCADE,
                embedding BLOB NOT NULL
            )",
            [],
        )?;
        Ok(())
    }

    fn insert_embedding(
        &self,
        conn: &Connection,
        symbol_id: i64,
        embedding: &[f32],
        dimensions: usize,
    ) -> Result<()> {
        validate_dimensions(embedding, dimensions)?;
        let bytes = serialize_embedding(embedding);
        conn.execute(
            "INSERT OR REPLACE INTO symbol_embeddings (symbol_id, embedding) VALUES (?, ?)",
            (symbol_id, bytes),
        )?;
        Ok(())
    }

    fn delete_embeddings(&self, conn: &Connection, symbol_ids: &[i64]) -> Result<()> {
        if symbol_ids.is_empty() {
            return Ok(());
        }
        let placeholders = repeat_placeholders(symbol_ids.len());
        let sql = format!("DELETE FROM symbol_embeddings WHERE symbol_id IN ({placeholders})");
        conn.execute(&sql, params_from_iter(symbol_ids.iter()))?;
        Ok(())
    }

    fn clear(&self, conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM symbol_embeddings", [])?;
        Ok(())
    }

    fn count(&self, conn: &Connection) -> Result<i64> {
        let count = conn.query_row("SELECT COUNT(*) FROM symbol_embeddings", [], |row| {
            row.get(0)
        })?;
        Ok(count)
    }

    fn search(
        &self,
        conn: &Connection,
        embedding: &[f32],
        dimensions: usize,
        limit: usize,
    ) -> Result<Vec<(i64, f64)>> {
        validate_dimensions(embedding, dimensions)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare("SELECT symbol_id, embedding FROM symbol_embeddings")?;
        let rows = stmt.query_map([], |row| {
            let symbol_id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((symbol_id, blob))
        })?;

        let mut scored = Vec::new();
        for row in rows {
            let (symbol_id, blob) = row?;
            let stored = deserialize_embedding(&blob, dimensions)?;
            scored.push((symbol_id, l2_distance(embedding, &stored)));
        }
        scored.sort_by(|left, right| left.1.total_cmp(&right.1));
        scored.truncate(limit);
        Ok(scored)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SqliteVecStore;

impl VectorStore for SqliteVecStore {
    fn backend_name(&self) -> &'static str {
        VectorBackend::SqliteVec.as_str()
    }

    fn create_schema(&self, conn: &Connection, dimensions: usize) -> Result<()> {
        validate_nonzero_dimensions(dimensions)?;
        let sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_symbols USING vec0(
                symbol_id INTEGER PRIMARY KEY,
                embedding float[{dimensions}]
            )"
        );
        conn.execute(&sql, [])?;
        Ok(())
    }

    fn insert_embedding(
        &self,
        conn: &Connection,
        symbol_id: i64,
        embedding: &[f32],
        dimensions: usize,
    ) -> Result<()> {
        validate_dimensions(embedding, dimensions)?;
        let bytes = serialize_embedding(embedding);
        conn.execute(
            "INSERT OR REPLACE INTO vec_symbols (symbol_id, embedding) VALUES (?, ?)",
            params![symbol_id, bytes],
        )?;
        Ok(())
    }

    fn delete_embeddings(&self, conn: &Connection, symbol_ids: &[i64]) -> Result<()> {
        if symbol_ids.is_empty() {
            return Ok(());
        }
        let placeholders = repeat_placeholders(symbol_ids.len());
        let sql = format!("DELETE FROM vec_symbols WHERE symbol_id IN ({placeholders})");
        conn.execute(&sql, params_from_iter(symbol_ids.iter()))?;
        Ok(())
    }

    fn clear(&self, conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM vec_symbols", [])?;
        Ok(())
    }

    fn count(&self, conn: &Connection) -> Result<i64> {
        let count = conn.query_row("SELECT COUNT(*) FROM vec_symbols", [], |row| row.get(0))?;
        Ok(count)
    }

    fn search(
        &self,
        conn: &Connection,
        embedding: &[f32],
        dimensions: usize,
        limit: usize,
    ) -> Result<Vec<(i64, f64)>> {
        validate_dimensions(embedding, dimensions)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let k = i64::try_from(limit)
            .map_err(|_| LoomError::InvalidInput("limit is too large".to_string()))?;
        let bytes = serialize_embedding(embedding);
        let mut stmt = conn.prepare(
            "SELECT symbol_id, distance
             FROM vec_symbols
             WHERE embedding MATCH ? AND k = ?
             ORDER BY distance",
        )?;
        let rows = stmt.query_map(params![bytes, k], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}

pub fn register_sqlite_vec_once() -> Result<()> {
    static REGISTRATION: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    REGISTRATION
        .get_or_init(|| {
            type AutoExtension = unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> c_int;
            // SAFETY: sqlite3_vec_init is the statically linked sqlite-vec extension
            // entrypoint. sqlite3_auto_extension stores the function pointer process-wide
            // and SQLite invokes it for future connections.
            let code = unsafe {
                let entry = std::mem::transmute::<*const (), AutoExtension>(
                    sqlite_vec::sqlite3_vec_init as *const (),
                );
                rusqlite::ffi::sqlite3_auto_extension(Some(entry))
            };
            if code == rusqlite::ffi::SQLITE_OK {
                Ok(())
            } else {
                Err(format!("sqlite3_auto_extension returned {code}"))
            }
        })
        .clone()
        .map_err(LoomError::VectorStore)
}

pub fn validate_dimensions(embedding: &[f32], dimensions: usize) -> Result<()> {
    validate_nonzero_dimensions(dimensions)?;
    if embedding.len() != dimensions {
        return Err(LoomError::VectorDimension {
            expected: dimensions,
            actual: embedding.len(),
        });
    }
    Ok(())
}

fn validate_nonzero_dimensions(dimensions: usize) -> Result<()> {
    if dimensions == 0 {
        return Err(LoomError::VectorDimension {
            expected: 1,
            actual: 0,
        });
    }
    Ok(())
}

pub fn repeat_placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(",")
}

fn serialize_embedding(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for value in embedding {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn deserialize_embedding(blob: &[u8], dimensions: usize) -> Result<Vec<f32>> {
    let expected = dimensions * 4;
    if blob.len() != expected {
        return Err(LoomError::VectorStore(format!(
            "stored embedding has {} bytes, expected {expected}",
            blob.len()
        )));
    }
    let mut values = Vec::with_capacity(dimensions);
    for chunk in blob.chunks_exact(4) {
        let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
        values.push(f32::from_le_bytes(bytes));
    }
    Ok(values)
}

fn l2_distance(left: &[f32], right: &[f32]) -> f64 {
    left.iter()
        .zip(right.iter())
        .map(|(l, r)| {
            let diff = f64::from(*l) - f64::from(*r);
            diff * diff
        })
        .sum::<f64>()
        .sqrt()
}
