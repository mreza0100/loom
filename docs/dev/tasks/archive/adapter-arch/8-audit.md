> Author: code-auditor

# Audit — adapter-arch

**Verdict:** NEEDS A SWEEP (no blocking issues)
**Date:** 2026-05-11

## Summary

11 findings, 0 critical, 0 blocking. Key items:

- **Strategy 5b confidence inversion** (pipeline.py:407-411) — pre-existing, returns 1.0 confidence for a low-confidence pattern. Needs /jc.
- **Stale tree-sitter deps** — 5 grammar packages installed but no adapters yet. Intentional forward-declaration for Wave 2 lang-adapters pipeline.
- **WATCH_EXTENSIONS frozen at import** — by design, documented in QA notes.
- **LIKE wildcard escaping** (pipeline.py:362, 394) — pre-existing, low risk.
- **Lazy Path import** (javascript.py:43) — trivial, should hoist to top.

## Blocking Issues

None.

## Recommended Follow-ups

1. `/jc`: Fix strategy 5b confidence score (pipeline.py:407-411)
2. `/jc`: Escape LIKE wildcards in pipeline.py strategies 2b/4b
3. `/jc`: Hoist Path import in javascript.py
4. Future: Move unused tree-sitter grammars to optional deps when adapters ship
