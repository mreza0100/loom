use crate::error::{LoomError, Result};
use rusqlite::{params_from_iter, Connection};

pub trait VectorStore: Send + Sync {
    fn create_schema(&self, conn: &Connection, dimensions: usize) -> Result<()>;
    fn insert_embedding(
        &self,
        conn: &Connection,
        symbol_id: i64,
        embedding: &[f32],
        dimensions: usize,
    ) -> Result<()>;
    fn delete_embeddings(&self, conn: &Connection, symbol_ids: &[i64]) -> Result<()>;
    fn count(&self, conn: &Connection) -> Result<i64>;
    fn search(
        &self,
        conn: &Connection,
        embedding: &[f32],
        dimensions: usize,
        limit: usize,
    ) -> Result<Vec<(i64, f64)>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BlobVectorStore;

#[derive(Debug, Clone, Copy, Default)]
pub struct SqliteVecVectorStore;

impl SqliteVecVectorStore {
    #[must_use]
    pub fn unavailable_reason() -> &'static str {
        "sqlite-vec is intentionally isolated behind VectorStore; BlobVectorStore is the foundation fallback"
    }
}

impl VectorStore for SqliteVecVectorStore {
    fn create_schema(&self, _conn: &Connection, _dimensions: usize) -> Result<()> {
        Err(LoomError::VectorStore(
            Self::unavailable_reason().to_string(),
        ))
    }

    fn insert_embedding(
        &self,
        _conn: &Connection,
        _symbol_id: i64,
        _embedding: &[f32],
        _dimensions: usize,
    ) -> Result<()> {
        Err(LoomError::VectorStore(
            Self::unavailable_reason().to_string(),
        ))
    }

    fn delete_embeddings(&self, _conn: &Connection, _symbol_ids: &[i64]) -> Result<()> {
        Err(LoomError::VectorStore(
            Self::unavailable_reason().to_string(),
        ))
    }

    fn count(&self, _conn: &Connection) -> Result<i64> {
        Err(LoomError::VectorStore(
            Self::unavailable_reason().to_string(),
        ))
    }

    fn search(
        &self,
        _conn: &Connection,
        _embedding: &[f32],
        _dimensions: usize,
        _limit: usize,
    ) -> Result<Vec<(i64, f64)>> {
        Err(LoomError::VectorStore(
            Self::unavailable_reason().to_string(),
        ))
    }
}

impl VectorStore for BlobVectorStore {
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

pub fn validate_dimensions(embedding: &[f32], dimensions: usize) -> Result<()> {
    if embedding.len() != dimensions {
        return Err(LoomError::VectorDimension {
            expected: dimensions,
            actual: embedding.len(),
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
