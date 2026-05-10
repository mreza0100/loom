"""Loom configuration."""

from dataclasses import dataclass, field
from pathlib import Path


@dataclass(frozen=True)
class LoomConfig:
    target_dir: Path
    db_path: Path = Path(".loom.db")
    watch_extensions: frozenset[str] = field(
        default_factory=lambda: frozenset({".js", ".ts", ".jsx", ".tsx", ".mjs", ".cjs"}),
    )
    debounce_seconds: float = 2.0
    embedding_model: str = "jinaai/jina-embeddings-v2-base-code"
    embedding_dimensions: int = 768
    max_file_size_bytes: int = 512_000
    excluded_dirs: frozenset[str] = field(
        default_factory=lambda: frozenset(
            {"node_modules", ".git", "dist", "build", ".next", "coverage", "__pycache__"},
        ),
    )

    def resolve_db_path(self) -> Path:
        return self.target_dir / self.db_path
