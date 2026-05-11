use crate::{config::LoomConfig, error::LoomError, Result};
use std::path::{Component, Path, PathBuf};

pub fn db_path_for(path: &Path, config: &LoomConfig) -> Result<String> {
    let relative = path.strip_prefix(&config.target_dir).map_err(|_| {
        LoomError::IndexerPath(format!(
            "{} is outside target dir {}",
            path.display(),
            config.target_dir.display()
        ))
    })?;
    Ok(normalize_path(relative))
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
