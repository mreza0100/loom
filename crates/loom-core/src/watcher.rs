use crate::{
    error::LoomError,
    indexer::{path, walk},
    LoomConfig, Result,
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::error;

pub trait ChangeHandler: Send + Sync {
    fn handle_changes(&self, paths: Vec<PathBuf>) -> Result<()>;
}

pub struct FnChangeHandler<F>
where
    F: Fn(Vec<PathBuf>) -> Result<()> + Send + Sync,
{
    callback: F,
}

impl<F> FnChangeHandler<F>
where
    F: Fn(Vec<PathBuf>) -> Result<()> + Send + Sync,
{
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> ChangeHandler for FnChangeHandler<F>
where
    F: Fn(Vec<PathBuf>) -> Result<()> + Send + Sync,
{
    fn handle_changes(&self, paths: Vec<PathBuf>) -> Result<()> {
        (self.callback)(paths)
    }
}

pub struct LoomWatcher {
    _watcher: RecommendedWatcher,
}

impl LoomWatcher {
    pub fn start(config: LoomConfig, handler: Arc<dyn ChangeHandler>) -> Result<Self> {
        let root = config.target_dir.clone();
        let debouncer = Arc::new(Mutex::new(Debouncer::new(config, handler)?));
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<Event>| match event {
                Ok(event) => {
                    let debounce_delay = match debouncer.lock() {
                        Ok(mut guard) => {
                            guard.handle_event(event);
                            guard.debounce_duration()
                        }
                        Err(source) => {
                            error!(error = %source, "watcher debouncer lock poisoned");
                            return;
                        }
                    };
                    let debouncer_for_flush = Arc::clone(&debouncer);
                    std::thread::spawn(move || {
                        std::thread::sleep(debounce_delay);
                        match debouncer_for_flush.lock() {
                            Ok(mut guard) => {
                                if guard.should_flush(Instant::now()) {
                                    guard.flush();
                                }
                            }
                            Err(source) => {
                                error!(error = %source, "watcher debouncer lock poisoned");
                            }
                        }
                    });
                }
                Err(source) => {
                    error!(error = %source, "file watcher event failed");
                }
            })
            .map_err(|source| LoomError::Watcher(source.to_string()))?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|source| LoomError::Watcher(source.to_string()))?;
        Ok(Self { _watcher: watcher })
    }
}

pub struct Debouncer {
    config: LoomConfig,
    handler: Arc<dyn ChangeHandler>,
    debounce: Duration,
    pending: BTreeSet<PathBuf>,
    hashes: BTreeMap<PathBuf, String>,
    last_event: Option<Instant>,
    loomignore: GlobSet,
}

impl Debouncer {
    pub fn new(config: LoomConfig, handler: Arc<dyn ChangeHandler>) -> Result<Self> {
        let debounce = Duration::from_secs_f64(config.debounce_seconds);
        let loomignore = load_loomignore(&config.target_dir)?;
        Ok(Self {
            config,
            handler,
            debounce,
            pending: BTreeSet::new(),
            hashes: BTreeMap::new(),
            last_event: None,
            loomignore,
        })
    }

    pub fn handle_event(&mut self, event: Event) {
        match event.kind {
            EventKind::Create(_) => {
                for path in event.paths {
                    self.force_enqueue(path);
                }
            }
            EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
                if let Some(path) = event.paths.last() {
                    self.force_enqueue(path.clone());
                }
            }
            EventKind::Modify(_) => {
                for path in event.paths {
                    self.enqueue_if_changed(path);
                }
            }
            EventKind::Remove(_) => {
                for path in event.paths {
                    self.enqueue_deleted(path);
                }
            }
            _ => {}
        }
    }

    pub fn should_flush(&self, now: Instant) -> bool {
        self.last_event
            .is_some_and(|last| now.duration_since(last) >= self.debounce)
            && !self.pending.is_empty()
    }

    pub fn debounce_duration(&self) -> Duration {
        self.debounce
    }

    pub fn flush(&mut self) {
        let batch = self.pending.iter().cloned().collect::<Vec<_>>();
        self.pending.clear();
        self.last_event = None;
        if let Err(source) = self.handler.handle_changes(batch) {
            error!(error = %source, "watcher callback failed");
        }
    }

    pub fn force_enqueue(&mut self, candidate: PathBuf) {
        if !self.accepts(&candidate, true) {
            return;
        }
        if let Ok(content_hash) = walk::hash_file(&candidate) {
            self.hashes.insert(candidate.clone(), content_hash);
        }
        self.pending.insert(candidate);
        self.last_event = Some(Instant::now());
    }

    pub fn enqueue_if_changed(&mut self, candidate: PathBuf) {
        if !self.accepts(&candidate, false) {
            return;
        }
        let Ok(content_hash) = walk::hash_file(&candidate) else {
            return;
        };
        if self.hashes.get(&candidate) == Some(&content_hash) {
            return;
        }
        self.hashes.insert(candidate.clone(), content_hash);
        self.pending.insert(candidate);
        self.last_event = Some(Instant::now());
    }

    pub fn enqueue_deleted(&mut self, candidate: PathBuf) {
        if !self.accepts_deleted(&candidate) {
            return;
        }
        self.hashes.remove(&candidate);
        self.pending.insert(candidate);
        self.last_event = Some(Instant::now());
    }

    pub fn pending_paths(&self) -> Vec<PathBuf> {
        self.pending.iter().cloned().collect()
    }

    fn accepts(&self, candidate: &Path, force: bool) -> bool {
        if candidate.is_dir() || self.ignored(candidate) {
            return false;
        }
        if force {
            candidate.exists() && path::should_index(candidate, &self.config)
        } else {
            path::should_index(candidate, &self.config)
        }
    }

    fn accepts_deleted(&self, candidate: &Path) -> bool {
        !self.ignored(candidate)
            && candidate
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| {
                    self.config
                        .watch_extensions
                        .contains(&format!(".{extension}"))
                })
                .unwrap_or(false)
    }

    fn ignored(&self, candidate: &Path) -> bool {
        if candidate
            .components()
            .filter_map(|component| match component {
                std::path::Component::Normal(part) => Some(part.to_string_lossy()),
                _ => None,
            })
            .any(|part| self.config.excluded_dirs.contains(part.as_ref()))
        {
            return true;
        }
        let relative = candidate
            .strip_prefix(&self.config.target_dir)
            .unwrap_or(candidate);
        self.loomignore.is_match(relative)
    }
}

fn load_loomignore(target_dir: &Path) -> Result<GlobSet> {
    let ignore_path = target_dir.join(".loomignore");
    let mut builder = GlobSetBuilder::new();
    if let Ok(raw) = std::fs::read_to_string(ignore_path) {
        for line in raw.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let glob = Glob::new(line).map_err(|source| LoomError::Watcher(source.to_string()))?;
            builder.add(glob);
        }
    }
    builder
        .build()
        .map_err(|source| LoomError::Watcher(source.to_string()))
}
