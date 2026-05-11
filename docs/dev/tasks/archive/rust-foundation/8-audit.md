# Pipeline Audit — rust-foundation

## Initial Verdict

NEEDS A SWEEP.

## Findings

| ID | Severity | Area | Finding | Resolution |
|---|---|---|---|---|
| AUDIT-RF-001 | SECURITY MEDIUM | Store limits | `search_fts` and `get_top_cochanges` cast unbounded `usize` limits to `i64`, allowing overflow into effectively unlimited SQLite reads. | Added `checked_sql_limit` with max `1_000`, returning `LoomError::InvalidInput`; added tests. |
| AUDIT-RF-002 | SECURITY LOW | PRAGMA helper | `reader_pragma_value(&str)` interpolated arbitrary PRAGMA identifiers. | Replaced string API with `ReaderPragma` enum whitelist. |
| AUDIT-RF-003 | SECURITY MEDIUM | Secrets hygiene | `.gitignore` lacked broad secret patterns. | Added `.env.*`, `*.pem`, `*.key`, `credentials.json`, and `secrets.*`. |
| AUDIT-RF-004 | STALE-DEP LOW | Dependencies | `time` was declared but unused. | Removed unused dependency. |
| AUDIT-RF-005 | SMELL LOW | Vector backend | Public `SqliteVecVectorStore` was constructible but always failed. | Removed always-failing public stub; kept `VectorStore` boundary and `BlobVectorStore` fallback. |

## Final Verification

```text
cargo build --workspace -> PASS
cargo test --workspace -> PASS, 15 Rust tests
cargo fmt --all -- --check -> PASS
cargo clippy --workspace -- -D warnings -> PASS
uv run pytest --tb=short -> PASS, 855 passed, coverage 91.69%
uv run ruff format --check -> PASS
uv run ruff check -> PASS
uv run mypy src -> PASS
```

## Final Verdict

PASS — audit findings resolved.

