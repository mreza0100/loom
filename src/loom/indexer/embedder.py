"""Code embedding using fastembed (ONNX-based, no PyTorch needed)."""

import logging
from typing import TYPE_CHECKING

from loom.config import LoomConfig

if TYPE_CHECKING:
    pass

log = logging.getLogger(__name__)


class Embedder:
    def __init__(self, config: LoomConfig) -> None:
        self._config = config
        self._model: object | None = None

    def _load_model(self) -> None:
        if self._model is not None:
            return
        from fastembed import TextEmbedding

        log.info("Loading embedding model: %s", self._config.embedding_model)
        self._model = TextEmbedding(
            self._config.embedding_model,
            providers=["CPUExecutionProvider"],
            enable_cpu_mem_arena=False,
        )
        log.info("Embedding model loaded")

    def embed(self, texts: list[str]) -> list[list[float]]:
        self._load_model()
        if self._model is None:
            raise RuntimeError("Embedding model failed to load")
        from fastembed import TextEmbedding

        if not isinstance(self._model, TextEmbedding):
            raise RuntimeError(f"Unexpected model type: {type(self._model)}")
        embeddings = list(self._model.embed(texts, batch_size=32))
        return [e.tolist() for e in embeddings]

    def embed_single(self, text: str) -> list[float]:
        return self.embed([text])[0]

    def build_symbol_text(self, name: str, kind: str, context: str) -> str:
        return f"{kind} {name}\n{context}"
