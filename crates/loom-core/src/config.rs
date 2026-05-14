use crate::error::{LoomError, Result};
use crate::parsers::AdapterRegistry;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const ALWAYS_EXCLUDED: [&str; 3] = [".git", "__pycache__", ".loom"];
const SUPPORTED_EMBEDDING_MODELS: [&str; 1] = ["jinaai/jina-embeddings-v2-base-code"];
const SIGNAL_EXTENSIONS: [&str; 4] = [".json", ".toml", ".yaml", ".yml"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VectorBackendConfig {
    SqliteVec,
    Blob,
}

impl VectorBackendConfig {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SqliteVec => "sqlite-vec",
            Self::Blob => "blob",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddingBackendConfig {
    Candle,
    Hashing,
}

impl EmbeddingBackendConfig {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Candle => "candle",
            Self::Hashing => "hashing",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoomConfig {
    pub target_dir: PathBuf,
    pub db_path: PathBuf,
    pub model_cache_dir: PathBuf,
    pub watch_extensions: BTreeSet<String>,
    pub debounce_seconds: f64,
    pub embedding_model: String,
    pub allow_custom_embedding_model: bool,
    pub vector_backend: VectorBackendConfig,
    pub embedding_backend: EmbeddingBackendConfig,
    pub allow_hashing_embedder_fallback: bool,
    pub auto_watch: bool,
    pub embedding_dimensions: usize,
    pub max_file_size_bytes: usize,
    pub excluded_dirs: BTreeSet<String>,
    pub structural_weight: f64,
    pub semantic_weight: f64,
    pub evolutionary_weight: f64,
    pub coupling_threshold: f64,
    pub top_coupled: usize,
    pub enable_git_analysis: bool,
    pub git_max_commits: usize,
    pub git_max_files_per_commit: usize,
}

#[derive(Debug, Deserialize)]
struct PartialConfig {
    db_path: Option<PathBuf>,
    model_cache_dir: Option<PathBuf>,
    watch_extensions: Option<Vec<String>>,
    debounce_seconds: Option<f64>,
    embedding_model: Option<String>,
    allow_custom_embedding_model: Option<bool>,
    vector_backend: Option<VectorBackendConfig>,
    embedding_backend: Option<EmbeddingBackendConfig>,
    allow_hashing_embedder_fallback: Option<bool>,
    auto_watch: Option<bool>,
    embedding_dimensions: Option<usize>,
    max_file_size_bytes: Option<usize>,
    excluded_dirs: Option<Vec<String>>,
    structural_weight: Option<f64>,
    semantic_weight: Option<f64>,
    evolutionary_weight: Option<f64>,
    coupling_threshold: Option<f64>,
    top_coupled: Option<usize>,
    enable_git_analysis: Option<bool>,
    git_max_commits: Option<usize>,
    git_max_files_per_commit: Option<usize>,
}

impl LoomConfig {
    #[must_use]
    pub fn default_for_target(target_dir: impl Into<PathBuf>) -> Self {
        let registry = AdapterRegistry::with_builtin_adapters();
        let mut watch_extensions = registry.get_all_extensions();
        Self::union_signal_extensions(&mut watch_extensions);
        let mut excluded_dirs = registry.get_all_excluded_dirs();
        Self::union_always_excluded(&mut excluded_dirs);

        Self {
            target_dir: target_dir.into(),
            db_path: PathBuf::from(".loom/loom.db"),
            model_cache_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".loom/models"),
            watch_extensions,
            debounce_seconds: 0.5,
            embedding_model: "jinaai/jina-embeddings-v2-base-code".to_string(),
            allow_custom_embedding_model: false,
            vector_backend: VectorBackendConfig::SqliteVec,
            embedding_backend: EmbeddingBackendConfig::Candle,
            allow_hashing_embedder_fallback: false,
            auto_watch: true,
            embedding_dimensions: 768,
            max_file_size_bytes: 512_000,
            excluded_dirs,
            structural_weight: 0.45,
            semantic_weight: 0.35,
            evolutionary_weight: 0.20,
            coupling_threshold: 0.30,
            top_coupled: 0,
            enable_git_analysis: true,
            git_max_commits: 500,
            git_max_files_per_commit: 20,
        }
    }

    pub fn load(target_dir: impl Into<PathBuf>) -> Result<Self> {
        let target_dir = target_dir.into();
        let config_path = target_dir.join(".loom/config.toml");
        let mut config = Self::default_for_target(target_dir);
        if !config_path.exists() {
            config.validate()?;
            return Ok(config);
        }

        let raw = fs::read_to_string(&config_path).map_err(|source| LoomError::ConfigIo {
            path: config_path.display().to_string(),
            source,
        })?;
        let partial: PartialConfig =
            toml::from_str(&raw).map_err(|source| LoomError::ConfigParse {
                path: config_path.display().to_string(),
                source,
            })?;

        config.apply_partial(partial);
        config.validate()?;
        Ok(config)
    }

    pub fn resolve_db_path(&self) -> Result<PathBuf> {
        let resolved = self.target_dir.join(&self.db_path);
        let parent = resolved.parent().ok_or_else(|| {
            LoomError::InvalidConfig(format!(
                "database path has no parent: {}",
                resolved.display()
            ))
        })?;
        fs::create_dir_all(parent).map_err(|source| LoomError::ConfigIo {
            path: parent.display().to_string(),
            source,
        })?;
        Ok(resolved)
    }

    #[must_use]
    pub fn embedding_fingerprint(&self) -> String {
        format!(
            "vector={};embedder={};model={};dims={};fallback={}",
            self.vector_backend.as_str(),
            self.embedding_backend.as_str(),
            self.embedding_model,
            self.embedding_dimensions,
            self.allow_hashing_embedder_fallback
        )
    }

    fn apply_partial(&mut self, partial: PartialConfig) {
        if let Some(db_path) = partial.db_path {
            self.db_path = db_path;
        }
        if let Some(model_cache_dir) = partial.model_cache_dir {
            self.model_cache_dir = model_cache_dir;
        }
        if let Some(watch_extensions) = partial.watch_extensions {
            self.watch_extensions = watch_extensions.into_iter().collect();
        }
        if let Some(debounce_seconds) = partial.debounce_seconds {
            self.debounce_seconds = debounce_seconds;
        }
        if let Some(embedding_model) = partial.embedding_model {
            self.embedding_model = embedding_model;
        }
        if let Some(allow_custom_embedding_model) = partial.allow_custom_embedding_model {
            self.allow_custom_embedding_model = allow_custom_embedding_model;
        }
        if let Some(vector_backend) = partial.vector_backend {
            self.vector_backend = vector_backend;
        }
        if let Some(embedding_backend) = partial.embedding_backend {
            self.embedding_backend = embedding_backend;
        }
        if let Some(allow_hashing_embedder_fallback) = partial.allow_hashing_embedder_fallback {
            self.allow_hashing_embedder_fallback = allow_hashing_embedder_fallback;
        }
        if let Some(auto_watch) = partial.auto_watch {
            self.auto_watch = auto_watch;
        }
        if let Some(embedding_dimensions) = partial.embedding_dimensions {
            self.embedding_dimensions = embedding_dimensions;
        }
        if let Some(max_file_size_bytes) = partial.max_file_size_bytes {
            self.max_file_size_bytes = max_file_size_bytes;
        }
        if let Some(excluded_dirs) = partial.excluded_dirs {
            self.excluded_dirs = excluded_dirs.into_iter().collect();
            Self::union_always_excluded(&mut self.excluded_dirs);
        }
        if let Some(structural_weight) = partial.structural_weight {
            self.structural_weight = structural_weight;
        }
        if let Some(semantic_weight) = partial.semantic_weight {
            self.semantic_weight = semantic_weight;
        }
        if let Some(evolutionary_weight) = partial.evolutionary_weight {
            self.evolutionary_weight = evolutionary_weight;
        }
        if let Some(coupling_threshold) = partial.coupling_threshold {
            self.coupling_threshold = coupling_threshold;
        }
        if let Some(top_coupled) = partial.top_coupled {
            self.top_coupled = top_coupled;
        }
        if let Some(enable_git_analysis) = partial.enable_git_analysis {
            self.enable_git_analysis = enable_git_analysis;
        }
        if let Some(git_max_commits) = partial.git_max_commits {
            self.git_max_commits = git_max_commits;
        }
        if let Some(git_max_files_per_commit) = partial.git_max_files_per_commit {
            self.git_max_files_per_commit = git_max_files_per_commit;
        }
    }

    fn validate(&self) -> Result<()> {
        for (name, weight) in [
            ("structural_weight", self.structural_weight),
            ("semantic_weight", self.semantic_weight),
            ("evolutionary_weight", self.evolutionary_weight),
        ] {
            if !weight.is_finite() {
                return Err(LoomError::InvalidConfig(format!("{name} must be finite")));
            }
            if weight < 0.0 {
                return Err(LoomError::InvalidConfig(
                    "coupling weights must be non-negative".to_string(),
                ));
            }
        }
        let active_total = self.structural_weight + self.semantic_weight + self.evolutionary_weight;
        if !active_total.is_finite() {
            return Err(LoomError::InvalidConfig(
                "coupling weight total must be finite".to_string(),
            ));
        }
        if active_total <= 0.0 {
            return Err(LoomError::InvalidConfig(
                "at least one coupling weight must be positive".to_string(),
            ));
        }
        if !self.coupling_threshold.is_finite() || !(0.0..=1.0).contains(&self.coupling_threshold) {
            return Err(LoomError::InvalidConfig(
                "coupling_threshold must be between 0.0 and 1.0".to_string(),
            ));
        }
        if self.embedding_dimensions == 0 {
            return Err(LoomError::InvalidConfig(
                "embedding_dimensions must be positive".to_string(),
            ));
        }
        if !self.allow_custom_embedding_model
            && !SUPPORTED_EMBEDDING_MODELS.contains(&self.embedding_model.as_str())
        {
            return Err(LoomError::InvalidConfig(format!(
                "embedding_model must be one of {} unless allow_custom_embedding_model = true",
                SUPPORTED_EMBEDDING_MODELS.join(", ")
            )));
        }
        if self.db_path == Path::new("") {
            return Err(LoomError::InvalidConfig(
                "db_path must not be empty".to_string(),
            ));
        }
        if self.db_path.is_absolute() {
            return Err(LoomError::InvalidConfig(
                "db_path must be relative to target_dir".to_string(),
            ));
        }
        if self
            .db_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(LoomError::InvalidConfig(
                "db_path must not escape target_dir".to_string(),
            ));
        }
        Ok(())
    }

    fn union_always_excluded(excluded_dirs: &mut BTreeSet<String>) {
        for dir in ALWAYS_EXCLUDED {
            excluded_dirs.insert(dir.to_string());
        }
    }

    fn union_signal_extensions(watch_extensions: &mut BTreeSet<String>) {
        for extension in SIGNAL_EXTENSIONS {
            watch_extensions.insert(extension.to_string());
        }
    }
}
