pub mod config;
pub mod embedder;
pub mod error;
pub mod git_analyzer;
pub mod graph;
pub mod indexer;
pub mod models;
pub mod parsers;
pub mod search;
pub mod store;
pub mod watcher;

pub use config::{EmbeddingBackendConfig, LoomConfig, VectorBackendConfig};
pub use embedder::{
    build_symbol_text, CandleEmbedder, DefaultEmbedder, Embedder, HashingEmbedder, ModelSource,
};
pub use error::{LoomError, Result};
pub use indexer::{EdgeResolver, IndexPipeline, IndexResult};
pub use parsers::{AdapterRegistry, LanguageAdapter, ParseResult};
pub use search::SearchEngine;
