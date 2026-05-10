"""Tests for loom.indexer.watcher — debounced file watcher."""

import time
from pathlib import Path
from unittest.mock import MagicMock

from loom.indexer.watcher import (
    DebouncedHandler,
    _hash_file,
    _is_excluded,
    start_watcher,
)


class TestHashFile:
    def test_hash_file_returns_hex(self, tmp_path: Path) -> None:
        f = tmp_path / "test.js"
        f.write_bytes(b"const x = 1;")
        result = _hash_file(f)
        assert isinstance(result, str)
        assert len(result) == 64  # sha256 hex

    def test_hash_file_consistent(self, tmp_path: Path) -> None:
        f = tmp_path / "test.js"
        f.write_bytes(b"function foo() {}")
        assert _hash_file(f) == _hash_file(f)

    def test_hash_file_changes_with_content(self, tmp_path: Path) -> None:
        f = tmp_path / "test.js"
        f.write_bytes(b"version 1")
        h1 = _hash_file(f)
        f.write_bytes(b"version 2")
        h2 = _hash_file(f)
        assert h1 != h2


class TestIsExcluded:
    def test_node_modules_excluded(self) -> None:
        path = Path("project/node_modules/pkg/index.js")
        assert _is_excluded(path) is True

    def test_git_excluded(self) -> None:
        path = Path("project/.git/HEAD")
        assert _is_excluded(path) is True

    def test_normal_path_not_excluded(self) -> None:
        path = Path("src/app.js")
        assert _is_excluded(path) is False

    def test_dist_excluded(self) -> None:
        path = Path("project/dist/bundle.js")
        assert _is_excluded(path) is True


class TestDebouncedHandlerEnqueue:
    def test_creates_with_defaults(self) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback)
        assert handler._debounce_sec == 2.0

    def test_custom_debounce(self) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=0.5)
        assert handler._debounce_sec == 0.5

    def test_enqueue_non_js_file_ignored(self, tmp_path: Path) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        event = MagicMock()
        event.is_directory = False
        event.src_path = str(tmp_path / "app.py")

        handler._enqueue(event)
        assert len(handler._pending) == 0

    def test_enqueue_directory_ignored(self) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        event = MagicMock()
        event.is_directory = True
        event.src_path = "/some/directory"

        handler._enqueue(event)
        assert len(handler._pending) == 0

    def test_enqueue_excluded_path_ignored(self, tmp_path: Path) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        nm = tmp_path / "node_modules" / "pkg"
        nm.mkdir(parents=True)
        f = nm / "index.js"
        f.write_text("module.exports = {};")

        event = MagicMock()
        event.is_directory = False
        event.src_path = str(f)

        handler._enqueue(event)
        assert len(handler._pending) == 0

    def test_enqueue_js_file_queued(self, tmp_path: Path) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        f = tmp_path / "app.js"
        f.write_text("function test() {}")

        event = MagicMock()
        event.is_directory = False
        event.src_path = str(f)

        handler._enqueue(event)
        assert Path(str(f)) in handler._pending

    def test_enqueue_same_hash_not_requeued(self, tmp_path: Path) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        f = tmp_path / "app.js"
        f.write_text("function test() {}")

        event = MagicMock()
        event.is_directory = False
        event.src_path = str(f)

        handler._enqueue(event)
        assert len(handler._pending) == 1

        # Enqueue same file again with same content — should not re-add
        handler._enqueue(event)
        assert len(handler._pending) == 1


class TestDebouncedHandlerEvents:
    def test_on_modified_delegates_to_enqueue(self, tmp_path: Path) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        f = tmp_path / "app.js"
        f.write_text("code")

        event = MagicMock()
        event.is_directory = False
        event.src_path = str(f)

        handler.on_modified(event)
        assert Path(str(f)) in handler._pending

    def test_on_created_queues_new_file(self, tmp_path: Path) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        f = tmp_path / "new.js"
        f.write_text("function brand_new() {}")

        event = MagicMock()
        event.is_directory = False
        event.src_path = str(f)

        handler.on_created(event)
        assert Path(str(f)) in handler._pending

    def test_on_created_ignores_directory(self) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        event = MagicMock()
        event.is_directory = True

        handler.on_created(event)
        assert len(handler._pending) == 0

    def test_on_created_ignores_non_js(self, tmp_path: Path) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        f = tmp_path / "readme.md"
        f.write_text("# README")

        event = MagicMock()
        event.is_directory = False
        event.src_path = str(f)

        handler.on_created(event)
        assert len(handler._pending) == 0

    def test_on_deleted_queues_path(self, tmp_path: Path) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        f = tmp_path / "deleted.js"

        event = MagicMock()
        event.is_directory = False
        event.src_path = str(f)

        handler.on_deleted(event)
        assert Path(str(f)) in handler._pending

    def test_on_deleted_ignores_directory(self) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        event = MagicMock()
        event.is_directory = True
        event.src_path = "/some/dir"

        handler.on_deleted(event)
        assert len(handler._pending) == 0

    def test_on_moved_queues_dest(self, tmp_path: Path) -> None:
        from watchdog.events import FileMovedEvent

        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        dest = tmp_path / "moved.js"
        dest.write_text("function moved() {}")

        event = FileMovedEvent(str(tmp_path / "original.js"), str(dest))

        handler.on_moved(event)
        assert dest in handler._pending

    def test_on_moved_ignores_directory(self) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        event = MagicMock()
        event.is_directory = True

        handler.on_moved(event)
        assert len(handler._pending) == 0


class TestDebouncedHandlerFlush:
    def test_flush_triggers_callback(self, tmp_path: Path) -> None:
        called: list[list[Path]] = []

        def callback(paths: list[Path]) -> None:
            called.append(paths)

        handler = DebouncedHandler(callback, debounce_sec=0.05)

        f = tmp_path / "app.js"
        f.write_text("code")

        event = MagicMock()
        event.is_directory = False
        event.src_path = str(f)
        handler._enqueue(event)

        # Wait for debounce
        time.sleep(0.2)
        assert len(called) == 1
        assert len(called[0]) == 1

    def test_flush_empty_pending_does_not_callback(self) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        handler._flush()
        callback.assert_not_called()

    def test_flush_clears_pending(self, tmp_path: Path) -> None:
        callback = MagicMock()
        handler = DebouncedHandler(callback, debounce_sec=100.0)

        f = tmp_path / "app.js"
        f.write_text("code")
        handler._pending.add(f)

        handler._flush()
        assert len(handler._pending) == 0

    def test_flush_callback_exception_logged(self) -> None:
        def bad_callback(paths: list[Path]) -> None:
            raise RuntimeError("callback failed")

        handler = DebouncedHandler(bad_callback, debounce_sec=100.0)
        handler._pending.add(Path("/some/file.js"))

        # Should not raise — exception is caught and logged
        handler._flush()


class TestStartWatcher:
    def test_start_watcher_returns_observer(self, tmp_path: Path) -> None:
        callback = MagicMock()
        observer = start_watcher(tmp_path, callback, debounce_sec=100.0)
        try:
            assert observer.is_alive()
        finally:
            observer.stop()
            observer.join(timeout=2)

    def test_start_watcher_with_custom_extensions(self, tmp_path: Path) -> None:
        callback = MagicMock()
        extensions = frozenset({".py"})
        observer = start_watcher(tmp_path, callback, extensions=extensions, debounce_sec=100.0)
        try:
            assert observer.is_alive()
        finally:
            observer.stop()
            observer.join(timeout=2)
