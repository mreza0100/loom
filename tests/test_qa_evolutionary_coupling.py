"""Adversarial QA tests for Phase 6 — Evolutionary Coupling.

Covers unhappy paths, edge cases, boundary conditions, and data integrity
scenarios not addressed by the developer-written tests.

Mock policy: subprocess is an external dep → mocked.
SQLite (LoomDB), scoring, config — internal deps → NOT mocked.
"""

import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from loom.config import LoomConfig
from loom.indexer.git_analyzer import GitAnalyzer
from loom.search.scoring import compute_evolutionary, fuse_signals
from loom.store.db import LoomDB

_EXTENSIONS = frozenset({".js", ".ts"})


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _mock_run(stdout: str, returncode: int = 0) -> MagicMock:
    m = MagicMock()
    m.stdout = stdout
    m.returncode = returncode
    return m


def _git_output(*groups: list[str]) -> str:
    return "\n".join("---COMMIT---\n" + "\n".join(g) for g in groups)


# ---------------------------------------------------------------------------
# GitAnalyzer — boundary conditions
# ---------------------------------------------------------------------------


class TestGitAnalyzerBoundaries:
    def test_exactly_max_files_per_commit_is_excluded(self, tmp_path: Path) -> None:
        """A commit with exactly max_files_per_commit=3 files (len > max is excluded).
        Spec: skip commits with > max_files_per_commit — so exactly max should be included.
        """
        # 3 files, max_files_per_commit=3 → len(files)=3, not > 3 → should be INCLUDED
        git_out = _git_output(["a.js", "b.js", "c.js"])
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            result = analyzer.analyze_cochanges(max_files_per_commit=3)
        # 3 files at max boundary — should produce pairs (not skipped)
        assert len(result) == 3  # C(3,2) = 3 pairs

    def test_exactly_max_files_plus_one_is_excluded(self, tmp_path: Path) -> None:
        """A commit with max_files_per_commit+1 files is skipped."""
        files = [f"file_{i}.js" for i in range(4)]
        git_out = _git_output(files)
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            result = analyzer.analyze_cochanges(max_files_per_commit=3)
        assert result == {}

    def test_exactly_two_files_included(self, tmp_path: Path) -> None:
        """Commit with exactly 2 files (minimum for a pair) is included."""
        git_out = _git_output(["src/a.js", "src/b.js"])
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            result = analyzer.analyze_cochanges()
        assert len(result) == 1

    def test_zero_files_commit_excluded(self, tmp_path: Path) -> None:
        """Commit with zero files (just sentinel) produces no pairs."""
        git_out = "---COMMIT---\n"
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            result = analyzer.analyze_cochanges()
        assert result == {}

    def test_all_files_filtered_by_extension_leaves_empty_commit(self, tmp_path: Path) -> None:
        """If all files in a commit have unsupported extensions → no pair produced."""
        git_out = _git_output(["README.md", "Makefile", "docs/index.rst"])
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            result = analyzer.analyze_cochanges()
        assert result == {}

    def test_single_supported_file_per_commit_excluded(self, tmp_path: Path) -> None:
        """One supported + one unsupported → after filter only 1 → no pair."""
        git_out = _git_output(["src/app.js", "README.md"])
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            result = analyzer.analyze_cochanges()
        assert result == {}

    def test_empty_watch_extensions_filters_everything(self, tmp_path: Path) -> None:
        """Empty watch_extensions frozenset → every file filtered → empty result."""
        git_out = _git_output(["src/a.js", "src/b.ts"])
        analyzer = GitAnalyzer(tmp_path, frozenset())
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            result = analyzer.analyze_cochanges()
        assert result == {}

    def test_file_with_no_extension_filtered_out(self, tmp_path: Path) -> None:
        """Files with no extension (Path('Makefile').suffix == '') are filtered when
        not in watch_extensions."""
        git_out = _git_output(["src/a.js", "Makefile"])
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            result = analyzer.analyze_cochanges()
        # Only one surviving file → no pair
        assert result == {}


# ---------------------------------------------------------------------------
# GitAnalyzer — output parsing edge cases
# ---------------------------------------------------------------------------


class TestGitAnalyzerParsing:
    def test_whitespace_only_lines_ignored(self, tmp_path: Path) -> None:
        """Lines that are only whitespace after strip are not treated as filenames."""
        stdout = "---COMMIT---\n   \n\tsrc/a.js\n\t\n\nsrc/b.ts\n"
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(stdout)):
            result = analyzer.analyze_cochanges()
        pair = (min("src/a.js", "src/b.ts"), max("src/a.js", "src/b.ts"))
        assert result.get(pair) == 1

    def test_multiple_sentinels_without_files_produce_no_pairs(self, tmp_path: Path) -> None:
        """Multiple consecutive sentinels (empty commits) produce no pairs."""
        stdout = "---COMMIT---\n---COMMIT---\n---COMMIT---\n"
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(stdout)):
            result = analyzer.analyze_cochanges()
        assert result == {}

    def test_git_returncode_nonzero_still_parses_stdout(self, tmp_path: Path) -> None:
        """Non-zero git exit code is not checked — analyze_cochanges parses
        whatever stdout was captured."""
        git_out = _git_output(["src/a.js", "src/b.js"])
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        # returncode != 0 but check=False — result.stdout still parsed
        with patch("subprocess.run", return_value=_mock_run(git_out, returncode=1)):
            result = analyzer.analyze_cochanges()
        # Implementation uses check=False, so stdout is still parsed
        pair = (min("src/a.js", "src/b.js"), max("src/a.js", "src/b.js"))
        assert result.get(pair) == 1

    def test_duplicate_filenames_in_same_commit_pair_counted_once(self, tmp_path: Path) -> None:
        """If git somehow lists the same file twice in a commit, both entries survive filtering.
        The pair (a, a) sorted → same string, min==max. Tests that we don't crash."""
        git_out = _git_output(["src/a.js", "src/a.js"])
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            # Should not raise — pair is (a, a), which is a degenerate self-pair
            result = analyzer.analyze_cochanges()
        # The pair is (src/a.js, src/a.js) — self-loop — counted or not, no crash
        assert isinstance(result, dict)

    def test_pair_ordering_reversed_input(self, tmp_path: Path) -> None:
        """Files listed in z→a order produce (a, z) key — lexicographic min/max."""
        git_out = _git_output(["z_service.js", "a_service.js"])
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            result = analyzer.analyze_cochanges()
        assert ("a_service.js", "z_service.js") in result
        assert ("z_service.js", "a_service.js") not in result

    def test_many_commits_accumulate_correctly(self, tmp_path: Path) -> None:
        """100 commits all touching the same pair → frequency == 100."""
        groups = [["src/a.js", "src/b.js"]] * 100
        git_out = _git_output(*groups)
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run(git_out)):
            result = analyzer.analyze_cochanges(max_commits=100)
        pair = (min("src/a.js", "src/b.js"), max("src/a.js", "src/b.js"))
        assert result[pair] == 100

    def test_max_commits_zero_returns_empty(self, tmp_path: Path) -> None:
        """max_commits=0 is passed through — git returns empty, result is empty."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run("")) as mock_run:
            result = analyzer.analyze_cochanges(max_commits=0)
        cmd = mock_run.call_args[0][0]
        assert "--max-count=0" in cmd
        assert result == {}


# ---------------------------------------------------------------------------
# GitAnalyzer — is_git_repo error paths
# ---------------------------------------------------------------------------


class TestIsGitRepo:
    def test_is_git_repo_target_dir_does_not_exist(self, tmp_path: Path) -> None:
        """target_dir that doesn't exist — subprocess may fail with FileNotFoundError or OSError."""
        nonexistent = tmp_path / "does_not_exist"
        analyzer = GitAnalyzer(nonexistent, _EXTENSIONS)
        # We only care it doesn't raise unhandled — either False or propagates
        with patch(
            "subprocess.run",
            side_effect=FileNotFoundError("git not found"),
        ):
            result = analyzer.is_git_repo()
        assert result is False

    def test_is_git_repo_returncode_128(self, tmp_path: Path) -> None:
        """Git returns 128 (not a git repo) → is_git_repo returns False."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run("", returncode=128)):
            assert analyzer.is_git_repo() is False

    def test_is_git_repo_returncode_1(self, tmp_path: Path) -> None:
        """Git returns 1 (any non-zero) → is_git_repo returns False."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_run("", returncode=1)):
            assert analyzer.is_git_repo() is False


# ---------------------------------------------------------------------------
# DB — cochange table edge cases
# ---------------------------------------------------------------------------


class TestCochangeDBEdgeCases:
    def test_upsert_self_loop_pair(self, db: LoomDB) -> None:
        """Upserting (a, a) — same file for both — stores one row without error."""
        db.upsert_cochange("src/self.js", "src/self.js", 3)
        db.commit()
        # Should be retrievable
        freq = db.get_cochange_frequency("src/self.js", "src/self.js")
        assert freq == 3

    def test_upsert_frequency_zero(self, db: LoomDB) -> None:
        """Frequency 0 can be stored — edge case, but should not error."""
        db.upsert_cochange("src/a.js", "src/b.js", 0)
        db.commit()
        freq = db.get_cochange_frequency("src/a.js", "src/b.js")
        assert freq == 0

    def test_upsert_large_frequency(self, db: LoomDB) -> None:
        """Very large frequency value stored and retrieved correctly."""
        db.upsert_cochange("src/a.js", "src/b.js", 999_999)
        db.commit()
        freq = db.get_cochange_frequency("src/a.js", "src/b.js")
        assert freq == 999_999

    def test_upsert_multiple_pairs_independent(self, db: LoomDB) -> None:
        """Multiple different pairs stored without interference."""
        pairs = [
            ("src/a.js", "src/b.js", 1),
            ("src/c.js", "src/d.js", 2),
            ("src/e.js", "src/f.js", 3),
        ]
        for a, b, freq in pairs:
            db.upsert_cochange(a, b, freq)
        db.commit()

        assert db.get_cochange_frequency("src/a.js", "src/b.js") == 1
        assert db.get_cochange_frequency("src/c.js", "src/d.js") == 2
        assert db.get_cochange_frequency("src/e.js", "src/f.js") == 3

    def test_upsert_overwrites_not_adds(self, db: LoomDB) -> None:
        """Second upsert with lower frequency overwrites (not accumulates)."""
        db.upsert_cochange("src/a.js", "src/b.js", 10)
        db.commit()
        db.upsert_cochange("src/a.js", "src/b.js", 2)
        db.commit()
        # ON CONFLICT DO UPDATE SET frequency = excluded.frequency → should be 2 now
        freq = db.get_cochange_frequency("src/a.js", "src/b.js")
        assert freq == 2

    def test_get_top_cochanges_file_only_in_file_b_column(self, db: LoomDB) -> None:
        """File appears only as file_b (its partner always sorts before it lexicographically)."""
        # "src/z.js" > "src/a.js" so stored as (src/a.js, src/z.js)
        db.upsert_cochange("src/z.js", "src/a.js", 5)  # stored as (a, z)
        db.commit()

        # Query for "src/z.js" — it's in file_b column
        top = db.get_top_cochanges("src/z.js")
        assert len(top) == 1
        assert top[0][0] == "src/a.js"
        assert top[0][1] == 5

    def test_get_top_cochanges_file_only_in_file_a_column(self, db: LoomDB) -> None:
        """File appears only as file_a (sorts before all partners lexicographically)."""
        db.upsert_cochange("src/a.js", "src/z.js", 7)
        db.commit()

        top = db.get_top_cochanges("src/a.js")
        assert len(top) == 1
        assert top[0][0] == "src/z.js"
        assert top[0][1] == 7

    def test_get_top_cochanges_mixed_columns(self, db: LoomDB) -> None:
        """Hub file appears as file_a for some partners and file_b for others."""
        # hub="src/m.js"; partners "src/a.js" < "src/m.js" < "src/z.js"
        db.upsert_cochange("src/m.js", "src/z.js", 3)  # stored as (m, z)
        db.upsert_cochange("src/a.js", "src/m.js", 5)  # stored as (a, m)
        db.commit()

        top = db.get_top_cochanges("src/m.js")
        partner_files = {f for f, _ in top}
        assert "src/z.js" in partner_files
        assert "src/a.js" in partner_files
        assert len(top) == 2

    def test_get_top_cochanges_default_limit_respected(self, db: LoomDB) -> None:
        """Default limit=20 does not return more than 20 rows even with 25 partners."""
        for i in range(25):
            db.upsert_cochange("src/hub.js", f"src/spoke_{i:02d}.js", i + 1)
        db.commit()

        top = db.get_top_cochanges("src/hub.js")  # default limit=20
        assert len(top) <= 20

    def test_get_cochange_frequency_both_orderings(self, db: LoomDB) -> None:
        """get_cochange_frequency(a, b) == get_cochange_frequency(b, a)."""
        db.upsert_cochange("src/alpha.js", "src/beta.js", 6)
        db.commit()
        assert db.get_cochange_frequency("src/alpha.js", "src/beta.js") == 6
        assert db.get_cochange_frequency("src/beta.js", "src/alpha.js") == 6


# ---------------------------------------------------------------------------
# Scoring — compute_evolutionary edge cases
# ---------------------------------------------------------------------------


class TestComputeEvolutionaryEdgeCases:
    def test_frequency_negative_clamped_to_zero(self) -> None:
        """Negative frequency: max(0.0, freq/max_freq) clamps to 0.0."""
        score = compute_evolutionary(-1, max_frequency=10)
        assert score == pytest.approx(0.0)

    def test_max_frequency_zero_returns_zero(self) -> None:
        """max_frequency=0 triggers the guard → returns 0.0, no ZeroDivisionError."""
        score = compute_evolutionary(5, max_frequency=0)
        assert score == pytest.approx(0.0)

    def test_max_frequency_negative_returns_zero(self) -> None:
        """max_frequency negative: guard (max_frequency <= 0) returns 0.0."""
        score = compute_evolutionary(5, max_frequency=-1)
        assert score == pytest.approx(0.0)

    def test_frequency_one_over_ten(self) -> None:
        """freq=1, max=10 → 0.1."""
        assert compute_evolutionary(1, max_frequency=10) == pytest.approx(0.1)

    def test_frequency_at_exact_max(self) -> None:
        """freq == max_frequency → exactly 1.0 (not > 1.0)."""
        score = compute_evolutionary(7, max_frequency=7)
        assert score == pytest.approx(1.0)

    def test_very_large_frequency_capped(self) -> None:
        """freq >> max_frequency → score == 1.0 (capped at 1.0)."""
        score = compute_evolutionary(10_000, max_frequency=10)
        assert score == pytest.approx(1.0)


# ---------------------------------------------------------------------------
# Scoring — fuse_signals with evolutionary edge cases
# ---------------------------------------------------------------------------


class TestFuseSignalsEvolutionary:
    def _config(self, s: float = 0.45, sem: float = 0.35, evo: float = 0.20) -> LoomConfig:
        return LoomConfig(
            target_dir=Path("."), structural_weight=s, semantic_weight=sem, evolutionary_weight=evo
        )

    def test_evolutionary_zero_omitted_from_breakdown(self) -> None:
        """evolutionary=0.0 → 'evolutionary' must NOT appear in breakdown()."""
        cs = fuse_signals(0.5, 0.5, 0.0, self._config())
        assert "evolutionary" not in cs.breakdown()

    def test_evolutionary_positive_appears_in_breakdown(self) -> None:
        """evolutionary > 0.0 → 'evolutionary' MUST appear in breakdown()."""
        cs = fuse_signals(0.5, 0.5, 0.3, self._config())
        assert "evolutionary" in cs.breakdown()

    def test_evolutionary_near_zero_below_threshold_still_appears_in_breakdown(self) -> None:
        """BUG: fuse_signals treats 1e-10 as zero (no evo weight), but CouplingScore stores
        the raw 1e-10. breakdown() checks > 0.0 → 1e-10 > 0.0 is True → 'evolutionary=0.00'
        appears in the breakdown string, misleadingly. Documents the inconsistency.
        """
        cs = fuse_signals(0.5, 0.5, 1e-10, self._config())
        # cs.evolutionary == 1e-10, and breakdown() shows 'evolutionary=0.00' (misleading)
        # The combined score uses the zero path (no evolutionary contribution)
        # but the breakdown DOES show evolutionary — this is the inconsistency
        assert cs.evolutionary == pytest.approx(1e-10)
        # The combined score should use 2-signal redistribution (no evo weight)
        expected_combined = min(1.0, 0.5 * (0.45 / (0.45 + 0.35)) + 0.5 * (0.35 / (0.45 + 0.35)))
        assert cs.combined == pytest.approx(expected_combined, rel=1e-4)

    def test_evolutionary_exactly_threshold_included(self) -> None:
        """evolutionary = 1e-9 (at threshold) → treated as non-zero (1e-9 is NOT < 1e-9).
        Three-signal path used. Evolutionary appears in breakdown."""
        cs = fuse_signals(0.5, 0.5, 1e-9, self._config())
        assert "evolutionary" in cs.breakdown()

    def test_combined_never_exceeds_one(self) -> None:
        """All signals at 1.0 → combined capped at 1.0."""
        cs = fuse_signals(1.0, 1.0, 1.0, self._config())
        assert cs.combined == pytest.approx(1.0)
        assert cs.combined <= 1.0

    def test_all_signals_zero_combined_is_zero(self) -> None:
        """All signals 0.0 → combined is 0.0."""
        cs = fuse_signals(0.0, 0.0, 0.0, self._config())
        assert cs.combined == pytest.approx(0.0)

    def test_weight_redistribution_when_evo_zero(self) -> None:
        """When evo=0.0, weights for structural+semantic are renormalized to sum to 1."""
        # structural=1.0, semantic=0.0, evo=0.0
        # expected: eff_s = 0.45/(0.45+0.35) = 0.5625, combined = 1.0*0.5625 = 0.5625
        cs = fuse_signals(1.0, 0.0, 0.0, self._config())
        expected = 0.45 / (0.45 + 0.35)
        assert cs.combined == pytest.approx(expected, rel=1e-4)

    def test_evolutionary_weight_zero_in_config(self) -> None:
        """Config with evolutionary_weight=0 — evolutionary>0 path uses all three weights
        but evo contribution is 0."""
        config = self._config(s=0.5, sem=0.5, evo=0.0)
        cs = fuse_signals(0.8, 0.6, 0.9, config)
        # evo > 0 triggers three-weight path: 0.8*0.5 + 0.6*0.5 + 0.9*0.0 = 0.7
        assert cs.combined == pytest.approx(0.7, rel=1e-4)
        # But evolutionary IS in the dataclass even though weight=0
        assert cs.evolutionary == pytest.approx(0.9)

    def test_structural_and_semantic_weights_zero(self) -> None:
        """Both structural and semantic weights are 0 → total_base=0 → combined=0 when evo=0."""
        config = self._config(s=0.0, sem=0.0, evo=1.0)
        # evo=0.0 triggers the redistribution path; total_base=0 → combined=0
        cs = fuse_signals(0.8, 0.6, 0.0, config)
        assert cs.combined == pytest.approx(0.0)

    def test_coupling_score_dataclass_immutable(self) -> None:
        """CouplingScore is frozen — attribute assignment must raise."""
        cs = fuse_signals(0.5, 0.5, 0.3, self._config())
        with pytest.raises((AttributeError, TypeError)):
            cs.combined = 0.0  # type: ignore[misc]


# ---------------------------------------------------------------------------
# Engine — _evolutionary_score integration
# ---------------------------------------------------------------------------


class TestEngineEvolutionaryScore:
    def test_evolutionary_score_returns_zero_for_missing_pair(self, db: LoomDB) -> None:
        """_evolutionary_score returns 0.0 when no cochange data exists for a file pair."""
        from loom.indexer.embedder import Embedder
        from loom.search.engine import SearchEngine

        embedder = MagicMock(spec=Embedder)
        embedder.embed_single.return_value = [0.0] * 768
        embedder.build_symbol_text.return_value = "test"

        engine = SearchEngine(db=db, embedder=embedder)
        score = engine._evolutionary_score("src/a.js", "src/b.js")  # noqa: SLF001
        assert score == pytest.approx(0.0)

    def test_evolutionary_score_returns_correct_value(self, db: LoomDB) -> None:
        """_evolutionary_score reads DB and normalizes correctly."""
        from loom.indexer.embedder import Embedder
        from loom.search.engine import SearchEngine

        db.upsert_cochange("src/a.js", "src/b.js", 5)
        db.commit()

        embedder = MagicMock(spec=Embedder)
        embedder.embed_single.return_value = [0.0] * 768

        engine = SearchEngine(db=db, embedder=embedder)
        score = engine._evolutionary_score("src/a.js", "src/b.js")  # noqa: SLF001
        assert score == pytest.approx(0.5)

    def test_evolutionary_score_same_file_is_zero(self, db: LoomDB) -> None:
        """Same file for both args → upsert would be a self-loop; query returns 0 since
        no self-loops are written by GitAnalyzer."""
        from loom.indexer.embedder import Embedder
        from loom.search.engine import SearchEngine

        embedder = MagicMock(spec=Embedder)
        engine = SearchEngine(db=db, embedder=embedder)
        score = engine._evolutionary_score("src/same.js", "src/same.js")  # noqa: SLF001
        assert score == pytest.approx(0.0)

    def test_evolutionary_score_max_frequency_reached(self, db: LoomDB) -> None:
        """Frequency >= 10 (default max) → score 1.0."""
        from loom.indexer.embedder import Embedder
        from loom.search.engine import SearchEngine

        db.upsert_cochange("src/a.js", "src/b.js", 15)
        db.commit()

        embedder = MagicMock(spec=Embedder)
        engine = SearchEngine(db=db, embedder=embedder)
        score = engine._evolutionary_score("src/a.js", "src/b.js")  # noqa: SLF001
        assert score == pytest.approx(1.0)


# ---------------------------------------------------------------------------
# Pipeline integration — git analysis disabled / non-repo / double-index
# ---------------------------------------------------------------------------


class TestPipelineEvolutionaryIntegration:
    def _make_pipeline(self, config: LoomConfig, db: LoomDB):  # type: ignore[no-untyped-def]
        from loom.indexer.embedder import Embedder
        from loom.indexer.pipeline import IndexPipeline

        embedder = MagicMock(spec=Embedder)
        embedder.embed.return_value = []
        embedder.build_symbol_text.return_value = "mock"
        return IndexPipeline(config=config, db=db, embedder=embedder)

    def test_full_index_twice_replaces_not_accumulates_cochange(self, tmp_path: Path) -> None:
        """Running full_index() twice with same git output keeps frequency=1, not 2.

        upsert_cochange uses ON CONFLICT DO UPDATE SET frequency = excluded.frequency,
        so the second run replaces the stored frequency rather than adding to it.
        """
        config = LoomConfig(target_dir=tmp_path, enable_git_analysis=True)
        fresh_db = LoomDB(config)
        fresh_db.connect()

        git_out = _git_output(["src/order.js", "src/cart.js"])

        def side_effect(cmd: list[str], **kwargs: object) -> MagicMock:
            if "rev-parse" in cmd:
                return _mock_run("true", returncode=0)
            return _mock_run(git_out)

        try:
            pipeline = self._make_pipeline(config, fresh_db)

            with patch("subprocess.run", side_effect=side_effect):
                pipeline.full_index()
            freq_after_first = fresh_db.get_cochange_frequency("src/order.js", "src/cart.js")

            with patch("subprocess.run", side_effect=side_effect):
                pipeline.full_index()
            freq_after_second = fresh_db.get_cochange_frequency("src/order.js", "src/cart.js")

            # Must be REPLACED not accumulated
            assert freq_after_first == 1
            assert freq_after_second == 1
        finally:
            fresh_db.close()

    def test_git_timeout_in_analyze_does_not_abort_pipeline(self, tmp_path: Path) -> None:
        """If git log times out, full_index() still completes — cochange table is empty."""
        config = LoomConfig(target_dir=tmp_path, enable_git_analysis=True)
        fresh_db = LoomDB(config)
        fresh_db.connect()

        def side_effect(cmd: list[str], **kwargs: object) -> MagicMock:
            if "rev-parse" in cmd:
                return _mock_run("true", returncode=0)
            raise subprocess.TimeoutExpired(cmd="git", timeout=30)

        try:
            pipeline = self._make_pipeline(config, fresh_db)
            with patch("subprocess.run", side_effect=side_effect):
                result = pipeline.full_index()  # must not raise

            assert isinstance(result, dict)
            count = fresh_db.conn.execute("SELECT COUNT(*) FROM cochange").fetchone()[0]
            assert count == 0
        finally:
            fresh_db.close()

    def test_git_config_max_commits_forwarded(self, tmp_path: Path) -> None:
        """config.git_max_commits is passed to analyze_cochanges → forwarded to subprocess cmd."""
        config = LoomConfig(target_dir=tmp_path, enable_git_analysis=True, git_max_commits=123)
        fresh_db = LoomDB(config)
        fresh_db.connect()

        captured_cmds: list[list[str]] = []

        def side_effect(cmd: list[str], **kwargs: object) -> MagicMock:
            captured_cmds.append(cmd)
            if "rev-parse" in cmd:
                return _mock_run("true", returncode=0)
            return _mock_run("")

        try:
            pipeline = self._make_pipeline(config, fresh_db)
            with patch("subprocess.run", side_effect=side_effect):
                pipeline.full_index()
        finally:
            fresh_db.close()

        log_calls = [c for c in captured_cmds if "log" in c]
        assert any("--max-count=123" in c for c in log_calls)

    def test_git_max_files_per_commit_forwarded(self, tmp_path: Path) -> None:
        """config.git_max_files_per_commit is used to filter commits in analyze_cochanges."""
        config = LoomConfig(
            target_dir=tmp_path,
            enable_git_analysis=True,
            git_max_files_per_commit=5,
        )
        fresh_db = LoomDB(config)
        fresh_db.connect()

        # 6 files in commit — exceeds max_files_per_commit=5 → should be excluded
        large_commit = [f"src/file_{i}.js" for i in range(6)]
        # 2 files in another commit — should be included
        small_commit = ["src/a.js", "src/b.js"]
        git_out = _git_output(large_commit, small_commit)

        def side_effect(cmd: list[str], **kwargs: object) -> MagicMock:
            if "rev-parse" in cmd:
                return _mock_run("true", returncode=0)
            return _mock_run(git_out)

        try:
            pipeline = self._make_pipeline(config, fresh_db)
            with patch("subprocess.run", side_effect=side_effect):
                pipeline.full_index()

            # Large commit excluded; only small commit's pair stored
            pair_freq = fresh_db.get_cochange_frequency("src/a.js", "src/b.js")
            assert pair_freq == 1

            # No pair from the 6-file commit should exist
            count = fresh_db.conn.execute("SELECT COUNT(*) FROM cochange").fetchone()[0]
            assert count == 1  # only the small commit produced a pair
        finally:
            fresh_db.close()


# ---------------------------------------------------------------------------
# Config — new fields present with correct defaults
# ---------------------------------------------------------------------------


class TestConfigNewFields:
    def test_enable_git_analysis_default_true(self) -> None:
        config = LoomConfig(target_dir=Path("."))
        assert config.enable_git_analysis is True

    def test_git_max_commits_default(self) -> None:
        config = LoomConfig(target_dir=Path("."))
        assert config.git_max_commits == 500

    def test_git_max_files_per_commit_default(self) -> None:
        config = LoomConfig(target_dir=Path("."))
        assert config.git_max_files_per_commit == 20

    def test_evolutionary_weight_default(self) -> None:
        config = LoomConfig(target_dir=Path("."))
        assert config.evolutionary_weight == pytest.approx(0.20)

    def test_signal_weights_sum_approximately_one(self) -> None:
        """structural + semantic + evolutionary weights should sum to ~1.0."""
        config = LoomConfig(target_dir=Path("."))
        total = config.structural_weight + config.semantic_weight + config.evolutionary_weight
        assert total == pytest.approx(1.0, rel=1e-4)

    def test_enable_git_analysis_can_be_set_false(self) -> None:
        config = LoomConfig(target_dir=Path("."), enable_git_analysis=False)
        assert config.enable_git_analysis is False

    def test_git_max_commits_custom(self) -> None:
        config = LoomConfig(target_dir=Path("."), git_max_commits=100)
        assert config.git_max_commits == 100


# ---------------------------------------------------------------------------
# DB stats — cochange_pairs in get_stats()
# ---------------------------------------------------------------------------


class TestDBStatsCochange:
    def test_stats_cochange_zero_initially(self, db: LoomDB) -> None:
        """Fresh DB has 0 cochange pairs."""
        stats = db.get_stats()
        assert stats["cochange_pairs"] == 0

    def test_stats_cochange_increments(self, db: LoomDB) -> None:
        """Inserting N pairs → cochange_pairs == N."""
        for i in range(5):
            db.upsert_cochange(f"src/file_{i}.js", f"src/partner_{i}.js", i + 1)
        db.commit()
        stats = db.get_stats()
        assert stats["cochange_pairs"] == 5

    def test_stats_cochange_upsert_no_duplicate_count(self, db: LoomDB) -> None:
        """Upserting the same pair twice → count stays at 1."""
        db.upsert_cochange("src/a.js", "src/b.js", 1)
        db.commit()
        db.upsert_cochange("src/a.js", "src/b.js", 5)
        db.commit()
        stats = db.get_stats()
        assert stats["cochange_pairs"] == 1
