"""Adapter registry singleton — imports and registers all language adapters."""

from loom.indexer.adapters.base import AdapterRegistry
from loom.indexer.adapters.javascript import JavaScriptAdapter

REGISTRY: AdapterRegistry = AdapterRegistry()
REGISTRY.register(JavaScriptAdapter())
