use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: i64,
    pub end_line: i64,
    pub language: String,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedEdge {
    pub source_name: String,
    pub target_name: String,
    pub relationship: String,
    pub target_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: Option<i64>,
    pub source_id: i64,
    pub target_id: Option<i64>,
    pub target_name: String,
    pub target_file: Option<String>,
    pub relationship: String,
    pub confidence: f64,
    pub original_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileState {
    pub path: String,
    pub content_hash: String,
    pub last_indexed: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CouplingScore {
    pub structural: f64,
    pub semantic: f64,
    pub evolutionary: f64,
    pub combined: f64,
}

impl CouplingScore {
    #[must_use]
    pub fn breakdown(&self) -> String {
        let mut parts = vec![
            format!("structural={:.2}", self.structural),
            format!("semantic={:.2}", self.semantic),
        ];
        if self.evolutionary > 0.0 {
            parts.push(format!("evolutionary={:.2}", self.evolutionary));
        }
        parts.join(" + ")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoupledSymbol {
    pub symbol: Symbol,
    pub score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub symbol: Symbol,
    pub score: f64,
    pub coupled: Vec<CoupledSymbol>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreStats {
    pub symbols: i64,
    pub edges: i64,
    pub files: i64,
    pub vectors: i64,
    pub last_indexed: Option<String>,
    pub stale_files: i64,
    pub cochange_pairs: i64,
}

impl StoreStats {
    #[must_use]
    pub fn as_map(&self) -> BTreeMap<String, Option<String>> {
        BTreeMap::from([
            ("symbols".to_string(), Some(self.symbols.to_string())),
            ("edges".to_string(), Some(self.edges.to_string())),
            ("files".to_string(), Some(self.files.to_string())),
            ("vectors".to_string(), Some(self.vectors.to_string())),
            ("last_indexed".to_string(), self.last_indexed.clone()),
            (
                "stale_files".to_string(),
                Some(self.stale_files.to_string()),
            ),
            (
                "cochange_pairs".to_string(),
                Some(self.cochange_pairs.to_string()),
            ),
        ])
    }
}
