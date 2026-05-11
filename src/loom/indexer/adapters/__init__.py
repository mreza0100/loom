"""Adapter registry singleton — imports and registers all language adapters."""

import logging

from loom.indexer.adapters.base import AdapterRegistry, LanguageAdapter

log = logging.getLogger(__name__)

REGISTRY: AdapterRegistry = AdapterRegistry()

# Each adapter imported in try/except — missing grammar skips the adapter without
# killing the server. ImportError only; do not catch broad exceptions.

try:
    from loom.indexer.adapters.javascript import JavaScriptAdapter

    REGISTRY.register(JavaScriptAdapter())
except ImportError:
    log.warning("tree-sitter-javascript not available; JS/TS files will not be indexed")

try:
    from loom.indexer.adapters.python import PythonAdapter

    REGISTRY.register(PythonAdapter())
except ImportError:
    log.warning("tree-sitter-python not available; Python files will not be indexed")

try:
    from loom.indexer.adapters.go import GoAdapter

    REGISTRY.register(GoAdapter())
except ImportError:
    log.warning("tree-sitter-go not available; Go files will not be indexed")

try:
    from loom.indexer.adapters.java import JavaAdapter

    REGISTRY.register(JavaAdapter())
except ImportError:
    log.warning("tree-sitter-java not available; Java files will not be indexed")

try:
    from loom.indexer.adapters.rust import RustAdapter

    REGISTRY.register(RustAdapter())
except ImportError:
    log.warning("tree-sitter-rust not available; Rust files will not be indexed")

try:
    from loom.indexer.adapters.csharp import CSharpAdapter

    REGISTRY.register(CSharpAdapter())
except ImportError:
    log.warning("tree-sitter-c-sharp not available; C# files will not be indexed")


def get_adapter(extension: str) -> LanguageAdapter | None:
    return REGISTRY.get_adapter(extension)


def get_all_extensions() -> frozenset[str]:
    return REGISTRY.get_all_extensions()
