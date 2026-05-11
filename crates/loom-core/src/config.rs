use crate::error::{LoomError, Result};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const ALWAYS_EXCLUDED: [&str; 3] = [".git", "__pycache__", ".loom"];

#[derive(Debug, Clone, PartialEq)]
pub struct LoomConfig {
    pub target_dir: PathBuf,
    pub db_path: PathBuf,
    pub watch_extensions: BTreeSet<String>,
    pub debounce_seconds: f64,
    pub embedding_model: String,
    pub embedding_dimensions: usize,
    pub max_file_size_bytes: usize,
    pub excluded_dirs: BTreeSet<String>,
    pub structural_weight: f64,
    pub semantic_weight: f64,
    pub evolutionary_weight: f64,
    pub enable_git_analysis: bool,
    pub git_max_commits: usize,
    pub git_max_files_per_commit: usize,
}

#[derive(Debug, Deserialize)]
struct PartialConfig {
    db_path: Option<PathBuf>,
    watch_extensions: Option<Vec<String>>,
    debounce_seconds: Option<f64>,
    embedding_model: Option<String>,
    embedding_dimensions: Option<usize>,
    max_file_size_bytes: Option<usize>,
    excluded_dirs: Option<Vec<String>>,
    structural_weight: Option<f64>,
    semantic_weight: Option<f64>,
    evolutionary_weight: Option<f64>,
    enable_git_analysis: Option<bool>,
    git_max_commits: Option<usize>,
    git_max_files_per_commit: Option<usize>,
}

impl LoomConfig {
    #[must_use]
    pub fn default_for_target(target_dir: impl Into<PathBuf>) -> Self {
        let watch_extensions = [
            ".py", ".js", ".jsx", ".ts", ".tsx", ".go", ".java", ".rs", ".cs",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let mut excluded_dirs: BTreeSet<String> = [
            ".git",
            "__pycache__",
            ".loom",
            "node_modules",
            ".venv",
            "venv",
            "target",
            "dist",
            "build",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        Self::union_always_excluded(&mut excluded_dirs);

        Self {
            target_dir: target_dir.into(),
            db_path: PathBuf::from(".loom/loom.db"),
            watch_extensions,
            debounce_seconds: 2.0,
            embedding_model: "jinaai/jina-embeddings-v2-base-code".to_string(),
            embedding_dimensions: 768,
            max_file_size_bytes: 512_000,
            excluded_dirs,
            structural_weight: 0.45,
            semantic_weight: 0.35,
            evolutionary_weight: 0.20,
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

    fn apply_partial(&mut self, partial: PartialConfig) {
        if let Some(db_path) = partial.db_path {
            self.db_path = db_path;
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
        if self.embedding_dimensions == 0 {
            return Err(LoomError::InvalidConfig(
                "embedding_dimensions must be positive".to_string(),
            ));
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
}
