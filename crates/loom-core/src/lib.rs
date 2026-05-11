pub mod config;
pub mod embedder;
pub mod error;
pub mod git_analyzer;
pub mod graph;
pub mod indexer;
pub mod models;
pub mod parsers;
pub mod store;
pub mod watcher;

pub use config::LoomConfig;
pub use embedder::{build_symbol_text, CandleEmbedder, Embedder, ModelSource};
pub use error::{LoomError, Result};
pub use indexer::{EdgeResolver, IndexPipeline, IndexResult};
pub use parsers::{AdapterRegistry, LanguageAdapter, ParseResult};
