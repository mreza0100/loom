"""Debounced file watcher with content-hash dedup."""

import hashlib
import logging
import threading
from collections.abc import Callable
from pathlib import Path

from watchdog.events import (
    FileMovedEvent,
    FileSystemEvent,
    FileSystemEventHandler,
)
from watchdog.observers import Observer
from watchdog.observers.api import BaseObserver

log = logging.getLogger(__name__)

WATCH_EXTENSIONS = frozenset({".js", ".ts", ".jsx", ".tsx", ".mjs", ".cjs"})
EXCLUDED_DIRS = frozenset(
    {"node_modules", ".git", "dist", "build", ".next", "coverage", "__pycache__"},
)


def _hash_file(path: Path) -> str:
    data = path.read_bytes()
    return hashlib.sha256(data).hexdigest()


def _is_excluded(path: Path) -> bool:
    return any(part in EXCLUDED_DIRS for part in path.parts)


class DebouncedHandler(FileSystemEventHandler):
    def __init__(
        self,
        callback: Callable[[list[Path]], None],
        debounce_sec: float = 2.0,
        extensions: frozenset[str] = WATCH_EXTENSIONS,
    ) -> None:
        self._callback = callback
        self._debounce_sec = debounce_sec
        self._extensions = extensions
        self._pending: set[Path] = set()
        self._hashes: dict[Path, str] = {}
        self._lock = threading.Lock()
        self._timer: threading.Timer | None = None

    def on_modified(self, event: FileSystemEvent) -> None:
        self._enqueue(event)

    def on_created(self, event: FileSystemEvent) -> None:
        if event.is_directory:
            return
        path = Path(str(event.src_path))
        if path.suffix not in self._extensions or _is_excluded(path):
            return
        self._force_enqueue(path)

    def on_moved(self, event: FileSystemEvent) -> None:
        if event.is_directory:
            return
        if not isinstance(event, FileMovedEvent):
            return
        dest = Path(str(event.dest_path))
        if dest.suffix not in self._extensions or _is_excluded(dest):
            return
        self._force_enqueue(dest)

    def on_deleted(self, event: FileSystemEvent) -> None:
        if event.is_directory:
            return
        path = Path(str(event.src_path))
        if path.suffix not in self._extensions or _is_excluded(path):
            return
        with self._lock:
            self._hashes.pop(path, None)
            self._pending.add(path)
            self._reset_timer()

    def _force_enqueue(self, path: Path) -> None:
        try:
            new_hash = _hash_file(path)
        except OSError:
            return
        self._hashes[path] = new_hash
        with self._lock:
            self._pending.add(path)
            self._reset_timer()

    def _enqueue(self, event: FileSystemEvent) -> None:
        if event.is_directory:
            return
        path = Path(str(event.src_path))
        if path.suffix not in self._extensions or _is_excluded(path):
            return

        try:
            new_hash = _hash_file(path)
        except OSError:
            return

        if self._hashes.get(path) == new_hash:
            return

        self._hashes[path] = new_hash

        with self._lock:
            self._pending.add(path)
            self._reset_timer()

    def _reset_timer(self) -> None:
        if self._timer is not None:
            self._timer.cancel()
        self._timer = threading.Timer(self._debounce_sec, self._flush)
        self._timer.daemon = True
        self._timer.start()

    def _flush(self) -> None:
        with self._lock:
            batch = list(self._pending)
            self._pending.clear()
            self._timer = None
        if batch:
            log.info("Flushing %d changed files for reindex", len(batch))
            try:
                self._callback(batch)
            except Exception:
                log.exception("Reindex callback failed for batch of %d files", len(batch))


def start_watcher(
    root: Path,
    callback: Callable[[list[Path]], None],
    debounce_sec: float = 2.0,
    extensions: frozenset[str] = WATCH_EXTENSIONS,
) -> BaseObserver:
    handler = DebouncedHandler(callback, debounce_sec, extensions)
    observer = Observer()
    observer.schedule(handler, str(root), recursive=True)
    observer.daemon = True
    observer.start()
    log.info("File watcher started on %s (debounce=%.1fs)", root, debounce_sec)
    return observer
