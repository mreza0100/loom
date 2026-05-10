"""Tests for loom.search.scoring — CouplingScore, compute_*, fuse_signals."""

from pathlib import Path

import pytest

from loom.config import LoomConfig
from loom.search.scoring import (
    CouplingScore,
    compute_evolutionary,
    compute_semantic,
    compute_structural,
    fuse_signals,
)

# Default config for fusion tests — target_dir never used for I/O in scoring
DEFAULT_CONFIG = LoomConfig(
    target_dir=Path("."),
    structural_weight=0.45,
    semantic_weight=0.35,
    evolutionary_weight=0.20,
)


class TestComputeStructural:
    def test_structural_score_calls_vs_imports(self) -> None:
        """calls (weight 1.0) should score higher than imports (weight 0.5)."""
        calls_score = compute_structural("calls", 1.0, 1)
        imports_score = compute_structural("imports", 1.0, 1)
        assert calls_score > imports_score

    def test_structural_score_depth_decay(self) -> None:
        """depth 1 > depth 2 > depth 3 for same relationship and confidence."""
        d1 = compute_structural("calls", 1.0, 1)
        d2 = compute_structural("calls", 1.0, 2)
        d3 = compute_structural("calls", 1.0, 3)
        assert d1 > d2 > d3

    def test_structural_score_depth_decay_values(self) -> None:
        """Verify exact depth decay: 1/(2^(depth-1))."""
        assert compute_structural("calls", 1.0, 1) == pytest.approx(1.0)
        assert compute_structural("calls", 1.0, 2) == pytest.approx(0.5)
        assert compute_structural("calls", 1.0, 3) == pytest.approx(0.25)

    def test_structural_score_confidence_weighting(self) -> None:
        """confidence 1.0 should score higher than confidence 0.6 at same depth/relationship."""
        high = compute_structural("calls", 1.0, 1)
        low = compute_structural("calls", 0.6, 1)
        assert high > low

    def test_structural_score_capped_at_one(self) -> None:
        """Score must never exceed 1.0."""
        score = compute_structural("calls", 2.0, 1)  # hypothetical high confidence
        assert score <= 1.0

    def test_structural_score_unknown_relationship_uses_default(self) -> None:
        """Unknown relationship falls back to DEFAULT_RELATIONSHIP_WEIGHT=0.5."""
        score = compute_structural("some_unknown_rel", 1.0, 1)
        assert score == pytest.approx(0.5)

    def test_structural_score_all_relationships(self) -> None:
        """All known relationships produce positive scores at depth 1, conf 1.0."""
        rels = [
            "calls",
            "extends",
            "called_by",
            "extended_by",
            "instantiates",
            "imports",
            "imported_by",
            "co_located",
        ]
        for rel in rels:
            score = compute_structural(rel, 1.0, 1)
            assert 0.0 < score <= 1.0, f"Unexpected score for {rel}: {score}"


class TestComputeSemantic:
    def test_semantic_score_from_distance_zero(self) -> None:
        """Distance 0 → similarity 1.0."""
        assert compute_semantic(0.0) == pytest.approx(1.0)

    def test_semantic_score_from_distance_half(self) -> None:
        """Distance 0.5 → similarity 0.5."""
        assert compute_semantic(0.5) == pytest.approx(0.5)

    def test_semantic_score_from_distance_one(self) -> None:
        """Distance 1.0 → similarity 0.0."""
        assert compute_semantic(1.0) == pytest.approx(0.0)

    def test_semantic_score_clipped_at_zero(self) -> None:
        """Distance > 1.0 must not produce negative similarity."""
        assert compute_semantic(1.5) == pytest.approx(0.0)


class TestComputeEvolutionary:
    def test_evolutionary_zero_frequency(self) -> None:
        assert compute_evolutionary(0) == pytest.approx(0.0)

    def test_evolutionary_full_frequency(self) -> None:
        assert compute_evolutionary(10, max_frequency=10) == pytest.approx(1.0)

    def test_evolutionary_normalized(self) -> None:
        assert compute_evolutionary(5, max_frequency=10) == pytest.approx(0.5)

    def test_evolutionary_capped_at_one(self) -> None:
        assert compute_evolutionary(100, max_frequency=10) == pytest.approx(1.0)


class TestFuseSignals:
    def test_fuse_signals_structural_only(self) -> None:
        """structural=0.8, semantic=0, evolutionary=0 → combined uses redistributed weight."""
        cs = fuse_signals(0.8, 0.0, 0.0, DEFAULT_CONFIG)
        # effective_structural_w = 0.45 / 0.80 = 0.5625
        expected = 0.8 * (0.45 / 0.80)
        assert cs.combined == pytest.approx(expected, rel=1e-4)
        assert cs.structural == pytest.approx(0.8)
        assert cs.semantic == pytest.approx(0.0)

    def test_fuse_signals_semantic_only(self) -> None:
        """structural=0, semantic=0.6, evolutionary=0 → combined uses redistributed weight."""
        cs = fuse_signals(0.0, 0.6, 0.0, DEFAULT_CONFIG)
        # effective_semantic_w = 0.35 / 0.80 = 0.4375
        expected = 0.6 * (0.35 / 0.80)
        assert cs.combined == pytest.approx(expected, rel=1e-4)

    def test_fuse_signals_both(self) -> None:
        """Both structural and semantic non-zero, no evolutionary."""
        cs = fuse_signals(0.8, 0.6, 0.0, DEFAULT_CONFIG)
        eff_s = 0.45 / 0.80
        eff_sem = 0.35 / 0.80
        expected = 0.8 * eff_s + 0.6 * eff_sem
        assert cs.combined == pytest.approx(min(1.0, expected), rel=1e-4)

    def test_fuse_signals_with_evolutionary(self) -> None:
        """When evolutionary > 0, all three configured weights are used directly."""
        cs = fuse_signals(0.8, 0.6, 0.5, DEFAULT_CONFIG)
        expected = 0.8 * 0.45 + 0.6 * 0.35 + 0.5 * 0.20
        assert cs.combined == pytest.approx(min(1.0, expected), rel=1e-4)
        assert cs.evolutionary == pytest.approx(0.5)

    def test_score_capped_at_one(self) -> None:
        """Combined score must not exceed 1.0 regardless of inputs."""
        cs = fuse_signals(1.0, 1.0, 1.0, DEFAULT_CONFIG)
        assert cs.combined <= 1.0

    def test_evolutionary_zero_redistributes_weight(self) -> None:
        """With evolutionary=0, structural+semantic at max = 1.0 (not 0.80)."""
        cs = fuse_signals(1.0, 1.0, 0.0, DEFAULT_CONFIG)
        # Both at max → combined should be 1.0 after redistribution
        assert cs.combined == pytest.approx(1.0)

    def test_evolutionary_zero_uses_full_weight(self) -> None:
        """Redistribution ensures 100% of available weight is used, not 80%."""
        # structural=1.0, semantic=0, evolutionary=0
        cs_no_evo = fuse_signals(1.0, 0.0, 0.0, DEFAULT_CONFIG)
        # If evolutionary weight were simply dropped, combined = 1.0 * 0.45 = 0.45
        # With redistribution: combined = 1.0 * (0.45/0.80) ≈ 0.5625
        assert cs_no_evo.combined > 0.45  # greater than raw structural_weight


class TestCouplingScoreBreakdown:
    def test_coupling_score_breakdown_string(self) -> None:
        cs = CouplingScore(structural=0.85, semantic=0.42, evolutionary=0.0, combined=0.70)
        breakdown = cs.breakdown()
        assert "structural" in breakdown
        assert "0.85" in breakdown
        assert "semantic" in breakdown
        assert "0.42" in breakdown
        # evolutionary=0, should be omitted
        assert "evolutionary" not in breakdown

    def test_breakdown_includes_evolutionary_when_nonzero(self) -> None:
        cs = CouplingScore(structural=0.7, semantic=0.3, evolutionary=0.5, combined=0.55)
        breakdown = cs.breakdown()
        assert "evolutionary" in breakdown
        assert "0.50" in breakdown

    def test_breakdown_format_matches_expected(self) -> None:
        cs = CouplingScore(structural=0.85, semantic=0.42, evolutionary=0.0, combined=0.70)
        assert cs.breakdown() == "structural=0.85 + semantic=0.42"

    def test_breakdown_format_with_evolutionary(self) -> None:
        cs = CouplingScore(structural=0.85, semantic=0.42, evolutionary=0.30, combined=0.60)
        assert cs.breakdown() == "structural=0.85 + semantic=0.42 + evolutionary=0.30"

    def test_structural_in_breakdown_for_compatibility(self) -> None:
        """test_structural_results_capped asserts 'structural' in c.reason — must pass."""
        cs = CouplingScore(structural=0.5, semantic=0.3, evolutionary=0.0, combined=0.45)
        assert "structural" in cs.breakdown()


class TestCouplingScoreImmutability:
    def test_coupling_score_is_frozen(self) -> None:
        cs = CouplingScore(structural=0.5, semantic=0.3, evolutionary=0.0, combined=0.45)
        with pytest.raises((AttributeError, TypeError)):
            cs.structural = 1.0  # type: ignore[misc]
