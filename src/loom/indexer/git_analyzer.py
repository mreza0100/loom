"""Git co-change analyzer — mines git log for file-level evolutionary coupling.

Extracts file co-change pairs from commit history using subprocess + git CLI.
No git Python libraries required — git log output is simple text.

Known limitation: git outputs paths relative to the repo root. The pipeline stores
file paths relative to target_dir via path.relative_to(config.target_dir). When
target_dir is a subdirectory of the repo, git paths carry an extra prefix and will not
match stored paths — evolutionary scores degrade to 0.0 for all pairs. This is a
Phase 6 scope constraint; full support requires mapping git root to target_dir.
"""

import logging
import subprocess
from collections import Counter
from pathlib import Path

log = logging.getLogger(__name__)

_COMMIT_SENTINEL = "---COMMIT---"


class GitAnalyzer:
    """Analyzes a git repository for file-level co-change pairs."""

    def __init__(self, target_dir: Path, watch_extensions: frozenset[str]) -> None:
        self._target_dir = target_dir
        self._watch_extensions = watch_extensions

    def is_git_repo(self) -> bool:
        """Return True if target_dir is inside a git repository.

        Runs ``git rev-parse --is-inside-work-tree``. Returns False if git is
        not on PATH (FileNotFoundError) or if the exit code is non-zero.
        """
        try:
            result = subprocess.run(  # noqa: S603
                ["git", "rev-parse", "--is-inside-work-tree"],  # noqa: S607
                capture_output=True,
                text=True,
                cwd=str(self._target_dir),
                check=False,
            )
            return result.returncode == 0
        except FileNotFoundError:
            log.warning("git not found on PATH — evolutionary coupling disabled")
            return False

    def analyze_cochanges(
        self,
        max_commits: int = 500,
        max_files_per_commit: int = 20,
    ) -> dict[tuple[str, str], int]:
        """Mine git log for file-level co-change pairs.

        Runs ``git log --max-count={max_commits} --name-only --pretty=format:---COMMIT---``
        and parses stdout into a frequency map of file pairs.

        Filtering rules (applied per commit):
        - Skip commits with < 2 files (no pair possible).
        - Skip commits with > max_files_per_commit files (noisy merge commits).
        - Skip files whose suffix is not in watch_extensions.

        Pair ordering: always (min(a, b), max(a, b)) for consistent deduplication.

        Returns:
            dict mapping (file_a, file_b) tuples to co-change frequency counts.
            Returns empty dict on timeout; re-raises on other subprocess errors.
        """
        cmd = [
            "git",
            "log",
            f"--max-count={max_commits}",
            "--name-only",
            f"--pretty=format:{_COMMIT_SENTINEL}",
        ]
        try:
            result = subprocess.run(  # noqa: S603
                cmd,
                capture_output=True,
                text=True,
                timeout=30,
                cwd=str(self._target_dir),
                check=False,
            )
        except subprocess.TimeoutExpired:
            log.warning(
                "git log timed out after 30s — skipping evolutionary coupling for %s",
                self._target_dir,
            )
            return {}
        except Exception:
            log.exception("Unexpected error running git log in %s", self._target_dir)
            raise

        counter: Counter[tuple[str, str]] = Counter()
        blocks = result.stdout.split(_COMMIT_SENTINEL)

        for block in blocks:
            files = [
                line.strip()
                for line in block.splitlines()
                if line.strip() and not line.strip().startswith(_COMMIT_SENTINEL)
            ]

            # Extension filter — only track configured file types
            files = [f for f in files if Path(f).suffix in self._watch_extensions]

            # Skip commits that cannot form a pair or are noisy mega-commits
            if len(files) < 2 or len(files) > max_files_per_commit:
                continue

            # Emit all O(n^2) pairs, sorted for canonical dedup ordering
            for i in range(len(files)):
                for j in range(i + 1, len(files)):
                    pair = (min(files[i], files[j]), max(files[i], files[j]))
                    counter[pair] += 1

        log.debug(
            "Git co-change analysis complete: %d unique pairs (max_commits=%d)",
            len(counter),
            max_commits,
        )
        return dict(counter)
