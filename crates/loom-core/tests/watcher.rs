use loom_core::{
    watcher::{ChangeHandler, Debouncer},
    LoomConfig, Result,
};
use notify::{event::CreateKind, event::ModifyKind, Event, EventKind};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

#[derive(Default)]
struct RecordingHandler {
    batches: Mutex<Vec<Vec<PathBuf>>>,
}

impl ChangeHandler for RecordingHandler {
    fn handle_changes(&self, paths: Vec<PathBuf>) -> Result<()> {
        self.batches.lock().unwrap().push(paths);
        Ok(())
    }
}

#[test]
fn debouncer_dedupes_same_content_modify_and_flushes_batch() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("app.py");
    fs::write(&file, "def alpha():\n    return 1\n").unwrap();
    let mut config = LoomConfig::default_for_target(dir.path());
    config.debounce_seconds = 0.0;
    let handler = Arc::new(RecordingHandler::default());
    let mut debouncer = Debouncer::new(config, handler.clone()).unwrap();

    debouncer.enqueue_if_changed(file.clone());
    debouncer.enqueue_if_changed(file.clone());
    assert_eq!(debouncer.pending_paths(), vec![file.clone()]);
    debouncer.flush();

    let batches = handler.batches.lock().unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0], vec![file]);
}

#[test]
fn debouncer_queues_create_delete_and_move_destination() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("created.py");
    let moved = dir.path().join("moved.py");
    fs::write(&file, "def alpha():\n    return 1\n").unwrap();
    fs::write(&moved, "def beta():\n    return 2\n").unwrap();
    let handler = Arc::new(RecordingHandler::default());
    let mut debouncer =
        Debouncer::new(LoomConfig::default_for_target(dir.path()), handler).unwrap();

    debouncer.handle_event(Event {
        kind: EventKind::Create(CreateKind::File),
        paths: vec![file.clone()],
        attrs: Default::default(),
    });
    debouncer.handle_event(Event {
        kind: EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both)),
        paths: vec![dir.path().join("old.py"), moved.clone()],
        attrs: Default::default(),
    });
    debouncer.enqueue_deleted(dir.path().join("gone.py"));

    assert_eq!(
        debouncer.pending_paths(),
        vec![
            file,
            dir.path().join("gone.py"),
            moved,
            dir.path().join("old.py")
        ]
    );
}

#[test]
fn debouncer_ignores_excluded_dirs_unsupported_extensions_and_loomignore() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".loomignore"), "ignored/**\n").unwrap();
    fs::create_dir(dir.path().join("ignored")).unwrap();
    fs::write(dir.path().join("ignored/app.py"), "def ignored(): pass\n").unwrap();
    fs::write(dir.path().join("notes.txt"), "not code\n").unwrap();
    fs::create_dir(dir.path().join("node_modules")).unwrap();
    fs::write(dir.path().join("node_modules/app.py"), "def nope(): pass\n").unwrap();
    let handler = Arc::new(RecordingHandler::default());
    let mut debouncer =
        Debouncer::new(LoomConfig::default_for_target(dir.path()), handler).unwrap();

    debouncer.force_enqueue(dir.path().join("ignored/app.py"));
    debouncer.force_enqueue(dir.path().join("notes.txt"));
    debouncer.force_enqueue(dir.path().join("node_modules/app.py"));

    assert!(debouncer.pending_paths().is_empty());
}
