"""LanguageAdapter protocol and AdapterRegistry for pluggable language support."""

from typing import Protocol, runtime_checkable

from loom.store.models import ParsedEdge, Symbol


@runtime_checkable
class LanguageAdapter(Protocol):
    """Protocol for language-specific parsing and module resolution.

    Each adapter handles one or more file extensions. It is responsible for:
    - Extracting symbols and edges from source bytes.
    - Resolving module import paths to actual indexed file paths.

    Implementors do not need to inherit from this class — structural subtyping applies.
    """

    extensions: frozenset[str]
    language_name: str
    excluded_dirs: frozenset[str]

    def parse(
        self,
        source: bytes,
        file_path: str,
    ) -> tuple[list[Symbol], list[ParsedEdge]]:
        """Extract symbols and edges from source bytes.

        Args:
            source: Raw file content as bytes. Never None.
            file_path: Absolute or relative path string for the file being parsed.

        Returns:
            A tuple of (symbols, edges). Both lists may be empty.
        """
        ...

    def resolve_module_path(
        self,
        import_path: str,
        source_file: str,
        known_files: set[str],
    ) -> str:
        """Resolve a module import path to a known indexed file path.

        Args:
            import_path: The module specifier from the import statement.
            source_file: The file containing the import (for relative resolution).
            known_files: Set of all currently indexed file paths.

        Returns:
            The resolved file path if found in known_files, otherwise import_path unchanged.
        """
        ...


class AdapterRegistry:
    """Registry mapping file extensions to LanguageAdapter instances.

    A single module-level instance (REGISTRY) is created in adapters/__init__.py.
    Consumers import REGISTRY from loom.indexer.adapters.
    """

    def __init__(self) -> None:
        self._by_extension: dict[str, LanguageAdapter] = {}
        self._adapters: list[LanguageAdapter] = []

    def register(self, adapter: LanguageAdapter) -> None:
        """Register an adapter. Maps each of its extensions to it."""
        for ext in adapter.extensions:
            self._by_extension[ext] = adapter
        if adapter not in self._adapters:
            self._adapters.append(adapter)

    def get_adapter(self, extension: str) -> LanguageAdapter | None:
        """Return the adapter for this file extension, or None."""
        return self._by_extension.get(extension)

    def get_all_extensions(self) -> frozenset[str]:
        """Union of extensions across all registered adapters."""
        result: set[str] = set()
        for adapter in self._adapters:
            result |= adapter.extensions
        return frozenset(result)

    def get_all_excluded_dirs(self) -> frozenset[str]:
        """Union of excluded_dirs across all registered adapters.

        Does NOT include .git or __pycache__ — those are consumer-layer concerns.
        """
        result: set[str] = set()
        for adapter in self._adapters:
            result |= adapter.excluded_dirs
        return frozenset(result)
