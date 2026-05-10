"""Coupling score computation — structural, semantic, evolutionary signal fusion."""

from dataclasses import dataclass

from loom.config import LoomConfig

# Weight of each relationship type in the structural signal.
# Unknown relationships fall back to DEFAULT_RELATIONSHIP_WEIGHT.
RELATIONSHIP_WEIGHT: dict[str, float] = {
    "calls": 1.0,
    "extends": 1.0,
    "called_by": 0.9,
    "extended_by": 0.9,
    "instantiates": 0.85,
    "imports": 0.5,
    "imported_by": 0.4,
    "co_located": 0.2,
}
DEFAULT_RELATIONSHIP_WEIGHT: float = 0.5


@dataclass(frozen=True)
class CouplingScore:
    """Immutable snapshot of signal scores for a single (source, target) pair."""

    structural: float
    semantic: float
    evolutionary: float
    combined: float

    def breakdown(self) -> str:
        """Human-readable signal decomposition.

        Format: "structural=0.85 + semantic=0.42"
        or:     "structural=0.85 + semantic=0.42 + evolutionary=0.30"

        The word "structural" always appears — required by test_structural_results_capped.
        Evolutionary is omitted when it is 0.0.
        """
        parts = [
            f"structural={self.structural:.2f}",
            f"semantic={self.semantic:.2f}",
        ]
        if self.evolutionary > 0.0:
            parts.append(f"evolutionary={self.evolutionary:.2f}")
        return " + ".join(parts)


def compute_structural(relationship: str, confidence: float, depth: int) -> float:
    """Compute structural coupling score.

    Formula: min(1.0, RELATIONSHIP_WEIGHT[relationship] × confidence × depth_decay)
    depth_decay = 1 / 2^(depth−1)  →  depth 1 = 1.0, depth 2 = 0.5, depth 3 = 0.25
    """
    rel_weight: float = RELATIONSHIP_WEIGHT.get(relationship, DEFAULT_RELATIONSHIP_WEIGHT)
    depth_decay: float = 1.0 / float(2 ** (depth - 1))
    return min(1.0, rel_weight * confidence * depth_decay)


def compute_semantic(distance: float) -> float:
    """Convert L2 distance to similarity score.

    Formula: max(0.0, 1.0 − distance)
    distance 0.0 → similarity 1.0, distance 1.0 → similarity 0.0.
    """
    return min(1.0, max(0.0, 1.0 - distance))


def compute_evolutionary(frequency: int, max_frequency: int = 10) -> float:
    """Normalize co-change frequency to [0, 1].

    Returns 0.0 in Phase 5 (no cochange data yet).  Function signature
    is forward-compatible with Phase 6 when real frequency data lands.

    Formula: min(1.0, frequency / max_frequency)
    """
    if max_frequency <= 0:
        return 0.0
    return min(1.0, max(0.0, frequency / max_frequency))


def fuse_signals(
    structural: float,
    semantic: float,
    evolutionary: float,
    config: LoomConfig,
) -> CouplingScore:
    """Fuse three coupling signals into a single CouplingScore.

    When evolutionary == 0.0, its weight is redistributed proportionally
    between structural and semantic so that combined still uses 100% of
    available weight (not 80%):

        total_base = structural_weight + semantic_weight
        effective_structural_w = structural_weight / total_base
        effective_semantic_w   = semantic_weight   / total_base

    When evolutionary > 0.0, all three configured weights are used directly:

        combined = structural × structural_weight
                 + semantic   × semantic_weight
                 + evolutionary × evolutionary_weight

    combined is capped at 1.0.
    """
    if evolutionary < 1e-9:
        total_base = config.structural_weight + config.semantic_weight
        if total_base <= 0.0:
            combined = 0.0
        else:
            eff_s = config.structural_weight / total_base
            eff_sem = config.semantic_weight / total_base
            combined = min(1.0, structural * eff_s + semantic * eff_sem)
    else:
        combined = min(
            1.0,
            structural * config.structural_weight
            + semantic * config.semantic_weight
            + evolutionary * config.evolutionary_weight,
        )

    return CouplingScore(
        structural=structural,
        semantic=semantic,
        evolutionary=evolutionary,
        combined=combined,
    )
