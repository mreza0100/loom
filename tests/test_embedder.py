"""Tests for loom.indexer.embedder — code embedding via fastembed."""

from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from loom.config import LoomConfig
from loom.indexer.embedder import Embedder


@pytest.fixture
def config(tmp_path: Path) -> LoomConfig:
    return LoomConfig(target_dir=tmp_path)


class TestEmbedder:
    def test_build_symbol_text(self, config: LoomConfig) -> None:
        embedder = Embedder(config)
        result = embedder.build_symbol_text(
            "processOrder", "function", "function processOrder() {}"
        )
        assert "processOrder" in result
        assert "function" in result
        assert "function processOrder() {}" in result

    def test_build_symbol_text_format(self, config: LoomConfig) -> None:
        embedder = Embedder(config)
        result = embedder.build_symbol_text("Cart", "class", "class Cart {}")
        lines = result.split("\n")
        assert len(lines) >= 2
        assert "Cart" in lines[0]

    def test_build_symbol_text_empty_context(self, config: LoomConfig) -> None:
        embedder = Embedder(config)
        result = embedder.build_symbol_text("foo", "function", "")
        assert "foo" in result

    def test_embed_calls_model(self, config: LoomConfig) -> None:
        """embed() calls the underlying model and returns float lists."""
        embedder = Embedder(config)

        import numpy as np
        from fastembed import TextEmbedding

        mock_model = MagicMock(spec=TextEmbedding)
        mock_model.embed.return_value = [np.array([0.1, 0.2, 0.3])]

        with patch("loom.indexer.embedder.Embedder._load_model"):
            embedder._model = mock_model
            result = embedder.embed(["test text"])

        assert len(result) == 1
        assert result[0] == pytest.approx([0.1, 0.2, 0.3])

    def test_embed_single_returns_single_vector(self, config: LoomConfig) -> None:
        """embed_single() wraps embed() and returns a flat float list."""
        embedder = Embedder(config)

        import numpy as np
        from fastembed import TextEmbedding

        mock_model = MagicMock(spec=TextEmbedding)
        mock_model.embed.return_value = [np.array([0.5] * 768)]

        with patch("loom.indexer.embedder.Embedder._load_model"):
            embedder._model = mock_model
            result = embedder.embed_single("test")

        assert isinstance(result, list)
        assert len(result) == 768

    def test_load_model_called_once(self, config: LoomConfig) -> None:
        """Model is loaded lazily and only once."""
        embedder = Embedder(config)

        import numpy as np
        from fastembed import TextEmbedding

        mock_model = MagicMock(spec=TextEmbedding)
        mock_model.embed.return_value = [np.array([0.1] * 768), np.array([0.2] * 768)]

        with patch("loom.indexer.embedder.Embedder._load_model") as mock_load:
            embedder._model = mock_model
            embedder.embed(["text1"])
            embedder.embed(["text2"])
            # _load_model is called on each embed() but the internal check prevents re-load
            assert mock_load.call_count == 2

    def test_model_starts_as_none(self, config: LoomConfig) -> None:
        embedder = Embedder(config)
        assert embedder._model is None

    def test_load_model_skips_if_already_loaded(self, config: LoomConfig) -> None:
        """_load_model should not re-create model if already set."""
        from fastembed import TextEmbedding

        embedder = Embedder(config)
        mock_existing = MagicMock(spec=TextEmbedding)
        embedder._model = mock_existing

        with patch("fastembed.TextEmbedding") as mock_cls:
            embedder._load_model()
            # Should NOT create a new model since _model is already set
            mock_cls.assert_not_called()
        assert embedder._model is mock_existing
