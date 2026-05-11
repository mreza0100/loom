use crate::{config::LoomConfig, error::LoomError, Result};
use std::path::{Component, Path, PathBuf};

pub fn db_path_for(path: &Path, config: &LoomConfig) -> Result<String> {
    let target_dir = config
        .target_dir
        .canonicalize()
        .map_err(|source| LoomError::IndexerIo {
            path: config.target_dir.display().to_string(),
            source,
        })?;
    let candidate = canonical_candidate(path)?;
    let relative = candidate.strip_prefix(&target_dir).map_err(|_| {
        LoomError::IndexerPath(format!(
            "{} is outside target dir {}",
            path.display(),
            target_dir.display()
        ))
    })?;
    let normalized = normalize_path(relative);
    if normalized.is_empty() || normalized.split('/').any(|part| part == "..") {
        return Err(LoomError::IndexerPath(format!(
            "{} does not resolve to an indexable file path",
            path.display()
        )));
    }
    Ok(normalized)
}

pub fn normalize_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            Component::CurDir => None,
            Component::ParentDir => Some("..".to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn should_index(path: &Path, config: &LoomConfig) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    let extension = format!(".{extension}");
    if !config.watch_extensions.contains(&extension) {
        return false;
    }
    if path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .any(|part| config.excluded_dirs.contains(part.as_ref()))
    {
        return false;
    }
    match path.metadata() {
        Ok(metadata) => metadata.len() <= config.max_file_size_bytes as u64,
        Err(_) => false,
    }
}

pub fn resolve_import_path(import_path: &str, source_file: &str) -> String {
    let source_dir = Path::new(source_file)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let joined = source_dir.join(import_path);
    normalize_posix(&joined)
}

fn normalize_posix(path: &Path) -> String {
    let mut parts = Vec::<String>::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::ParentDir => {
                let _ = parts.pop();
            }
            Component::CurDir => {}
            _ => {}
        }
    }
    parts.join("/")
}

pub fn absolute_path(path: PathBuf, config: &LoomConfig) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        config.target_dir.join(path)
    }
}

fn canonical_candidate(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return path.canonicalize().map_err(|source| LoomError::IndexerIo {
            path: path.display().to_string(),
            source,
        });
    }
    let parent = path.parent().ok_or_else(|| {
        LoomError::IndexerPath(format!("{} has no parent directory", path.display()))
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| LoomError::IndexerPath(format!("{} has no file name", path.display())))?;
    let parent = parent
        .canonicalize()
        .map_err(|source| LoomError::IndexerIo {
            path: parent.display().to_string(),
            source,
        })?;
    Ok(parent.join(file_name))
}
