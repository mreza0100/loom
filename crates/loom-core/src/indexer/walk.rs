use crate::{config::LoomConfig, indexer::path, Result};
use ignore::WalkBuilder;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileJob {
    pub absolute_path: PathBuf,
    pub db_path: String,
    pub content_hash: String,
}

pub fn hash_file(path: &std::path::Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|source| crate::error::LoomError::IndexerIo {
        path: path.display().to_string(),
        source,
    })?;
    Ok(hash_bytes(&bytes))
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn discover_files(config: &LoomConfig) -> Vec<PathBuf> {
    let mut builder = WalkBuilder::new(&config.target_dir);
    builder
        .add_custom_ignore_filename(".loomignore")
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true)
        .hidden(false)
        .max_filesize(Some(config.max_file_size_bytes as u64))
        .threads(default_rayon_threads());
    let excluded = config.excluded_dirs.clone();
    builder.filter_entry(move |entry| {
        !entry
            .path()
            .components()
            .filter_map(|component| match component {
                std::path::Component::Normal(part) => Some(part.to_string_lossy()),
                _ => None,
            })
            .any(|part| excluded.contains(part.as_ref()))
    });

    builder
        .build()
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.into_path())
        .filter(|candidate| candidate.is_file() && path::should_index(candidate, config))
        .collect()
}

pub fn default_rayon_threads() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .saturating_sub(1)
        .clamp(1, 8)
}
