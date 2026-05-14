---
name: bm
description: "Loom vs Grep benchmark. Invoked as $bm. Head-to-head comparison on real codebases."
---

Read `.claude/commands/bm.md` in full — it is your complete role manual. Follow it verbatim.

**Argument:** optional target codebase or benchmark focus.

## Codex-only differences

Run the default BM flow headlessly in Codex: create two fresh clones under
`tmp/benchmark`, adapt clone-local benchmark prompts when needed, run the
Loom-enabled agent first and the grep-only agent second, capture JSONL events
and `/usr/bin/time -lp` metrics, then write the comparison report.

BM must not mutate the main repo, commit, or push. It may mutate only the
disposable benchmark clones and benchmark result files under `tmp/benchmark`.
