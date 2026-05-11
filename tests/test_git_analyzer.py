"""Tests for GitAnalyzer, LoomDB cochange methods, and evolutionary scoring integration.

All subprocess calls are mocked — git is an external dependency.
DB tests use the real SQLite DB via the conftest db fixture (internal dep, not mocked).
"""

import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from loom.config import LoomConfig
from loom.indexer.git_analyzer import GitAnalyzer
from loom.search.scoring import compute_evolutionary, fuse_signals
from loom.store.db import LoomDB

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_EXTENSIONS = frozenset({".js", ".ts", ".jsx", ".tsx"})


def _make_git_output(*commit_file_groups: list[str]) -> str:
    """Build git log --pretty=format:---COMMIT--- --name-only style output."""
    parts = []
    for files in commit_file_groups:
        parts.append("---COMMIT---\n" + "\n".join(files))
    return "\n".join(parts)


def _mock_subprocess_run(stdout: str, returncode: int = 0) -> MagicMock:
    mock_result = MagicMock()
    mock_result.stdout = stdout
    mock_result.returncode = returncode
    return mock_result


# ---------------------------------------------------------------------------
# TestGitAnalyzerCochangeExtraction
# ---------------------------------------------------------------------------


class TestGitAnalyzerCochangeExtraction:
    def test_git_cochange_extraction(self, tmp_path: Path) -> None:
        """Two commits, each with two overlapping files — verify pair counts."""
        git_output = _make_git_output(
            ["src/order.js", "src/validation.js"],
            ["src/order.js", "src/cart.ts"],
        )
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run(git_output)):
            result = analyzer.analyze_cochanges()

        # order.js + validation.js: 1 time
        assert result.get(("src/order.js", "src/validation.js")) == 1
        # order.js + cart.ts: 1 time
        assert result.get(("src/cart.ts", "src/order.js")) == 1
        # validation.js + cart.ts: never co-changed
        assert ("src/cart.ts", "src/validation.js") not in result

    def test_cochange_frequency_accumulates(self, tmp_path: Path) -> None:
        """Same pair in multiple commits — frequency accumulates."""
        git_output = _make_git_output(
            ["src/a.js", "src/b.js"],
            ["src/a.js", "src/b.js"],
            ["src/a.js", "src/b.js"],
        )
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run(git_output)):
            result = analyzer.analyze_cochanges()

        pair = (min("src/a.js", "src/b.js"), max("src/a.js", "src/b.js"))
        assert result[pair] == 3

    def test_large_commit_filtered(self, tmp_path: Path) -> None:
        """Commit with more than max_files_per_commit files is excluded."""
        many_files = [f"src/file_{i}.js" for i in range(25)]
        git_output = _make_git_output(many_files)
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run(git_output)):
            result = analyzer.analyze_cochanges(max_files_per_commit=20)

        assert result == {}

    def test_single_file_commit_filtered(self, tmp_path: Path) -> None:
        """Commit with < 2 files cannot form a pair — produces zero results."""
        git_output = _make_git_output(["src/lonely.js"])
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run(git_output)):
            result = analyzer.analyze_cochanges()

        assert result == {}

    def test_extension_filtering(self, tmp_path: Path) -> None:
        """Files with extensions not in watch_extensions are dropped; tracked files still pair."""
        git_output = _make_git_output(
            ["src/app.js", "README.md", "src/utils.ts"],
        )
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run(git_output)):
            result = analyzer.analyze_cochanges()

        # README.md should be filtered; .js and .ts survive and form a pair
        pair = (min("src/app.js", "src/utils.ts"), max("src/app.js", "src/utils.ts"))
        assert result.get(pair) == 1
        # No pair should involve README.md
        for key in result:
            assert "README.md" not in key

    def test_cochange_pair_ordering(self, tmp_path: Path) -> None:
        """Pairs are always stored as (min(a, b), max(a, b)) — lexicographic ordering."""
        git_output = _make_git_output(
            ["src/z.js", "src/a.js"],  # z > a — output key must be (a, z)
        )
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run(git_output)):
            result = analyzer.analyze_cochanges()

        assert ("src/a.js", "src/z.js") in result
        assert ("src/z.js", "src/a.js") not in result

    def test_not_a_git_repo(self, tmp_path: Path) -> None:
        """is_git_repo() returns False when git exits non-zero."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run("", returncode=128)):
            assert analyzer.is_git_repo() is False

    def test_git_not_on_path(self, tmp_path: Path) -> None:
        """is_git_repo() returns False when git is not on PATH (FileNotFoundError)."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", side_effect=FileNotFoundError):
            assert analyzer.is_git_repo() is False

    def test_git_is_repo_returns_true(self, tmp_path: Path) -> None:
        """is_git_repo() returns True when git rev-parse exits 0."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run("true", returncode=0)):
            assert analyzer.is_git_repo() is True

    def test_git_timeout(self, tmp_path: Path) -> None:
        """subprocess.TimeoutExpired causes analyze_cochanges to return {} without raising."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", side_effect=subprocess.TimeoutExpired(cmd="git", timeout=30)):
            result = analyzer.analyze_cochanges()

        assert result == {}

    def test_git_timeout_does_not_crash(self, tmp_path: Path) -> None:
        """Verify analyze_cochanges is safe to call even when subprocess times out."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", side_effect=subprocess.TimeoutExpired(cmd="git", timeout=30)):
            # Must not raise
            result = analyzer.analyze_cochanges(max_commits=100)
        assert isinstance(result, dict)

    def test_other_subprocess_exception_propagates(self, tmp_path: Path) -> None:
        """Non-timeout subprocess errors re-raise — we don't swallow exceptions."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with (
            patch("subprocess.run", side_effect=OSError("disk full")),
            pytest.raises(OSError, match="disk full"),
        ):
            analyzer.analyze_cochanges()

    def test_empty_git_output(self, tmp_path: Path) -> None:
        """Empty git output produces empty result without error."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run("")):
            result = analyzer.analyze_cochanges()

        assert result == {}

    def test_max_commits_arg_passed_to_subprocess(self, tmp_path: Path) -> None:
        """max_commits value is forwarded to the git command."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run("")) as mock_run:
            analyzer.analyze_cochanges(max_commits=42)

        call_args = mock_run.call_args
        cmd = call_args[0][0]
        assert "--max-count=42" in cmd

    def test_cwd_set_to_target_dir(self, tmp_path: Path) -> None:
        """subprocess.run is called with cwd=target_dir for git commands."""
        analyzer = GitAnalyzer(tmp_path, _EXTENSIONS)
        with patch("subprocess.run", return_value=_mock_subprocess_run("")) as mock_run:
            analyzer.analyze_cochanges()

        call_kwargs = mock_run.call_args[1]
        assert call_kwargs["cwd"] == str(tmp_path)


# ---------------------------------------------------------------------------
# TestCochangeDB
# ---------------------------------------------------------------------------


class TestCochangeDB:
    def test_upsert_cochange(self, db: LoomDB) -> None:
        """Inserting a new co-change pair stores it with the given frequency."""
        db.upsert_cochange("src/a.js", "src/b.js", 5)
        db.commit()
        freq = db.get_cochange_frequency("src/a.js", "src/b.js")
        assert freq == 5

    def test_upsert_cochange_updates_on_conflict(self, db: LoomDB) -> None:
        """Upserting the same pair twice replaces frequency — no duplicate rows."""
        db.upsert_cochange("src/a.js", "src/b.js", 3)
        db.commit()
        db.upsert_cochange("src/a.js", "src/b.js", 7)
        db.commit()
        freq = db.get_cochange_frequency("src/a.js", "src/b.js")
        assert freq == 7
        # Verify exactly one row exists
        count = db.conn.execute(
            "SELECT COUNT(*) FROM cochange WHERE file_a = 'src/a.js' AND file_b = 'src/b.js'"
        ).fetchone()[0]
        assert count == 1

    def test_upsert_cochange_canonical_ordering(self, db: LoomDB) -> None:
        """Upsert with (b, a) and then (a, b) lands as the same row."""
        db.upsert_cochange("src/z.js", "src/a.js", 2)
        db.commit()
        db.upsert_cochange("src/a.js", "src/z.js", 4)
        db.commit()
        freq = db.get_cochange_frequency("src/a.js", "src/z.js")
        assert freq == 4
        count = db.conn.execute("SELECT COUNT(*) FROM cochange").fetchone()[0]
        assert count == 1

    def test_get_cochange_frequency_missing(self, db: LoomDB) -> None:
        """Returns 0 for a pair that has never been stored."""
        freq = db.get_cochange_frequency("src/missing.js", "src/also_missing.js")
        assert freq == 0

    def test_get_cochange_frequency_exists(self, db: LoomDB) -> None:
        """Returns the correct frequency for a stored pair."""
        db.upsert_cochange("src/order.js", "src/cart.js", 12)
        db.commit()
        freq = db.get_cochange_frequency("src/order.js", "src/cart.js")
        assert freq == 12

    def test_get_cochange_frequency_reversed_args(self, db: LoomDB) -> None:
        """Argument order doesn't matter — (b, a) looks up same row as (a, b)."""
        db.upsert_cochange("src/a.js", "src/b.js", 9)
        db.commit()
        assert db.get_cochange_frequency("src/b.js", "src/a.js") == 9

    def test_get_top_cochanges(self, db: LoomDB) -> None:
        """Returns partner files ordered by frequency descending."""
        db.upsert_cochange("src/order.js", "src/cart.js", 10)
        db.upsert_cochange("src/order.js", "src/validation.js", 3)
        db.upsert_cochange("src/order.js", "src/product.js", 7)
        db.commit()

        top = db.get_top_cochanges("src/order.js", limit=10)
        files = [f for f, _ in top]
        freqs = [freq for _, freq in top]

        assert "src/cart.js" in files
        assert "src/validation.js" in files
        assert "src/product.js" in files
        # Ordered by frequency desc
        assert freqs == sorted(freqs, reverse=True)
        assert freqs[0] == 10

    def test_get_top_cochanges_limit(self, db: LoomDB) -> None:
        """get_top_cochanges respects the limit parameter."""
        for i in range(10):
            db.upsert_cochange("src/hub.js", f"src/spoke_{i}.js", i + 1)
        db.commit()

        top = db.get_top_cochanges("src/hub.js", limit=3)
        assert len(top) == 3

    def test_get_top_cochanges_empty(self, db: LoomDB) -> None:
        """Returns empty list when file has no co-change partners."""
        top = db.get_top_cochanges("src/orphan.js")
        assert top == []

    def test_get_stats_includes_cochange_pairs(self, db: LoomDB) -> None:
        """get_stats() reports cochange_pairs count."""
        db.upsert_cochange("src/a.js", "src/b.js", 1)
        db.upsert_cochange("src/c.js", "src/d.js", 2)
        db.commit()

        stats = db.get_stats()
        assert "cochange_pairs" in stats
        assert stats["cochange_pairs"] == 2


# ---------------------------------------------------------------------------
# TestEvolutionaryScoringIntegration
# ---------------------------------------------------------------------------


class TestEvolutionaryScoringIntegration:
    def test_evolutionary_score_freq_ten(self) -> None:
        """freq 10 → score 1.0 (at max_frequency=10 default)."""
        assert compute_evolutionary(10) == pytest.approx(1.0)

    def test_evolutionary_score_freq_five(self) -> None:
        """freq 5 → score 0.5."""
        assert compute_evolutionary(5) == pytest.approx(0.5)

    def test_evolutionary_score_freq_zero(self) -> None:
        """freq 0 → score 0.0."""
        assert compute_evolutionary(0) == pytest.approx(0.0)

    def test_evolutionary_score_over_max_capped(self) -> None:
        """freq > max_frequency → score capped at 1.0."""
        assert compute_evolutionary(100, max_frequency=10) == pytest.approx(1.0)

    def test_same_file_evolutionary_zero(self, db: LoomDB) -> None:
        """Same file co-change query always returns 0 (no self-loops in git)."""
        freq = db.get_cochange_frequency("src/same.js", "src/same.js")
        assert freq == 0

    def test_scoring_with_evolutionary_from_db(self, db: LoomDB) -> None:
        """fuse_signals with real cochange data from DB includes evolutionary in breakdown."""
        db.upsert_cochange("src/a.js", "src/b.js", 8)
        db.commit()

        freq = db.get_cochange_frequency("src/a.js", "src/b.js")
        evo_score = compute_evolutionary(freq)
        assert evo_score == pytest.approx(0.8)

        config = LoomConfig(
            target_dir=Path("."),
            structural_weight=0.45,
            semantic_weight=0.35,
            evolutionary_weight=0.20,
        )
        cs = fuse_signals(0.6, 0.5, evo_score, config)
        assert "evolutionary" in cs.breakdown()
        assert cs.evolutionary == pytest.approx(0.8)
        # All three weights active: combined should reflect evolutionary contribution
        expected = 0.6 * 0.45 + 0.5 * 0.35 + 0.8 * 0.20
        assert cs.combined == pytest.approx(min(1.0, expected), rel=1e-4)

    def test_scoring_without_evolutionary_redistributes_weight(self) -> None:
        """When evolutionary=0.0, weight redistributes to structural+semantic (full 100%)."""
        config = LoomConfig(
            target_dir=Path("."),
            structural_weight=0.45,
            semantic_weight=0.35,
            evolutionary_weight=0.20,
        )
        cs = fuse_signals(1.0, 1.0, 0.0, config)
        assert cs.combined == pytest.approx(1.0)
        assert "evolutionary" not in cs.breakdown()


# ---------------------------------------------------------------------------
# TestPipelineGitIntegration
# ---------------------------------------------------------------------------


class TestPipelineGitIntegration:
    def _make_pipeline(self, config: LoomConfig, db: LoomDB) -> "IndexPipeline":  # type: ignore[name-defined]  # noqa: F821
        from loom.indexer.embedder import Embedder
        from loom.indexer.pipeline import IndexPipeline

        embedder = MagicMock(spec=Embedder)
        embedder.embed.return_value = []
        embedder.build_symbol_text.return_value = "mock text"
        return IndexPipeline(config=config, db=db, embedder=embedder)

    def test_full_index_runs_git_analysis_when_enabled(self, tmp_path: Path, db: LoomDB) -> None:
        """Git analysis runs during full_index() when enable_git_analysis=True."""
        config = LoomConfig(target_dir=tmp_path, enable_git_analysis=True)
        # Use shared db fixture — it's already connected to config.resolve_db_path()
        # We need a fresh db pointing to tmp_path
        fresh_db = LoomDB(config)
        fresh_db.connect()

        git_output = _make_git_output(
            ["src/order.js", "src/cart.js"],
        )
        is_repo_result = _mock_subprocess_run("true", returncode=0)
        log_result = _mock_subprocess_run(git_output)

        try:
            pipeline = self._make_pipeline(config, fresh_db)

            def side_effect(cmd: list[str], **kwargs: object) -> MagicMock:  # type: ignore[type-arg]
                if "rev-parse" in cmd:
                    return is_repo_result
                return log_result

            with patch("subprocess.run", side_effect=side_effect):
                pipeline.full_index()

            # Cochange should be written to DB
            freq = fresh_db.get_cochange_frequency("src/order.js", "src/cart.js")
            assert freq == 1
        finally:
            fresh_db.close()

    def test_full_index_skips_git_analysis_when_disabled(self, tmp_path: Path) -> None:
        """When enable_git_analysis=False, subprocess.run is never called for git."""
        config = LoomConfig(target_dir=tmp_path, enable_git_analysis=False)
        fresh_db = LoomDB(config)
        fresh_db.connect()

        try:
            pipeline = self._make_pipeline(config, fresh_db)

            with patch("subprocess.run") as mock_run:
                pipeline.full_index()

            # subprocess.run should NOT have been called for git
            for call in mock_run.call_args_list:
                cmd = call[0][0] if call[0] else call[1].get("args", [])
                assert "git" not in cmd, f"git subprocess called unexpectedly: {cmd}"
        finally:
            fresh_db.close()

    def test_full_index_skips_when_not_git_repo(self, tmp_path: Path) -> None:
        """When is_git_repo() returns False, no cochange rows are written."""
        config = LoomConfig(target_dir=tmp_path, enable_git_analysis=True)
        fresh_db = LoomDB(config)
        fresh_db.connect()

        try:
            pipeline = self._make_pipeline(config, fresh_db)

            # rev-parse returns 128 → not a git repo
            with patch(
                "subprocess.run",
                return_value=_mock_subprocess_run("", returncode=128),
            ):
                pipeline.full_index()

            count = fresh_db.conn.execute("SELECT COUNT(*) FROM cochange").fetchone()[0]
            assert count == 0
        finally:
            fresh_db.close()
