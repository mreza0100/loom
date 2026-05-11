"""Thin dispatcher — delegates to the registered LanguageAdapter for each file extension."""

import logging
from pathlib import Path

from loom.indexer.adapters import REGISTRY
from loom.store.models import ParsedEdge, Symbol

log = logging.getLogger(__name__)


def parse_file(
    file_path: Path,
    source: bytes | None = None,
) -> tuple[list[Symbol], list[ParsedEdge]]:
    """Parse a source file and extract symbols and edges.

    Dispatches to the registered LanguageAdapter for file_path.suffix.
    Returns ([], []) for unknown extensions.

    Args:
        file_path: Path to the file. Used for extension lookup and debug logging.
        source: Raw file bytes. If None, the file is read from disk.

    Returns:
        A tuple of (symbols, edges).
    """
    if source is None:
        source = file_path.read_bytes()

    adapter = REGISTRY.get_adapter(file_path.suffix)
    if adapter is None:
        log.debug("No adapter for extension %s, skipping %s", file_path.suffix, file_path.name)
        return [], []

    return adapter.parse(source, str(file_path))
