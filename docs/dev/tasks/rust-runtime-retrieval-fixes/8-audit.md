# Pipeline Audit — rust-runtime-retrieval-fixes

**Pipeline:** rust-runtime-retrieval-fixes  
**Date:** 2026-05-11  
**Final verdict:** SPARKLING

## Audit History

The first audit found two actionable issues:

- Vector backend switches could leave the new backend with no embeddings while `index_meta` still marked files fresh.
- The new native dependency path needed a dependency-audit workflow.

The follow-up audit found additional freshness hardening gaps:

- Cochange rows were upserted without replacing stale rows from prior git-analysis windows.
- Embedding freshness used config identity rather than the actual active embedder, so explicit Candle-to-hashing fallback could mix vector spaces.
- The security workflow needed tighter pins and locked Python export.

All issues were fixed in follow-up implementation commits.

## Final Recheck

Final audit recheck after commit `893daf9` found no blocking issues:

- Active embedder identity now comes from the live `DefaultEmbedder`, including backend, degraded state, model, and dimensions.
- Index skip decisions use that active fingerprint.
- Freshness compares content hash, active embedding fingerprint, and active vector-backend row coverage.
- Writes persist the active embedder fingerprint into `index_meta`.
- Legacy DBs get `index_meta.embedding_fingerprint` added idempotently.
- Cochange rows are replaced transactionally during full git analysis.
- The security workflow pins actions/tools and uses locked `uv export`.

## Verification

Final gates passed:

| Check | Result |
|---|---|
| `cargo test --workspace` | PASS — 67 tests |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Focused audit recheck | PASS — 0 findings |

## Residual Notes

`cargo-llvm-cov` is not installed in this environment, so Rust line coverage was not collected. Python reference coverage remained above threshold during post-merge QA.

Audit complete. SPARKLING.
