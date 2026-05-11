use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoomError {
    #[error("configuration IO failed for {path}: {source}")]
    ConfigIo {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("configuration parse failed for {path}: {source}")]
    ConfigParse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("reader pool error: {0}")]
    Pool(#[from] r2d2::Error),
    #[error("vector dimension mismatch: expected {expected}, got {actual}")]
    VectorDimension { expected: usize, actual: usize },
    #[error("vector store error: {0}")]
    VectorStore(String),
    #[error("missing connection: {0}")]
    MissingConnection(String),
    #[error("graph lookup failed: {0}")]
    GraphLookup(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("parser language setup failed for {language}: {source}")]
    ParserLanguage {
        language: String,
        #[source]
        source: tree_sitter::LanguageError,
    },
    #[error("parser IO failed for {path}: {source}")]
    ParserIo {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parser produced no tree for {language} at {path}")]
    ParserNoTree { language: String, path: String },
}

pub type Result<T> = std::result::Result<T, LoomError>;
