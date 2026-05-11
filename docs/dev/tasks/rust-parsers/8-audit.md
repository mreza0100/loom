# Pipeline Audit — rust-parsers

## Initial Verdict

NEEDS A SWEEP.

## Findings

| ID | Severity | Area | Finding | Resolution |
|---|---|---|---|---|
| AUDIT-RP-001 | HIGH | Java adapter | Interface inheritance was emitted as `implements` instead of `extends`. | Distinguished `super_interfaces`/`interfaces` from `extends_interfaces`; added regression test. |
| AUDIT-RP-002 | MEDIUM | Go adapter | Typed var declarations could emit type names as variables. | Extracted declared identifiers only from `const_spec` / `var_spec`; added regression test. |
| AUDIT-RP-003 | MEDIUM | C# adapter | Base-list relationship used `I` naming heuristic for interfaces. | Removed heuristic and defaulted base-list edges to `extends`; added regression test. |
| AUDIT-RP-004 | MEDIUM | JS/TS adapter | Class fields were mislabeled as methods. | Split class fields from method definitions and emit fields as `variable`; added regression test. |
| AUDIT-RP-005 | LOW | Tests | Parser tests were too smoke-heavy for several parity cases. | Added focused QA tests for the four parity regressions. |
| AUDIT-RP-006 | LOW | Public API | Root crate re-exported `parse_file`, which can read arbitrary paths if wired incorrectly later. | Removed root `parse_file` re-export; parser module still exposes it for internal/future indexer use. |

## Final Verification

```text
cargo build --workspace -> PASS
cargo test --workspace -> PASS, 30 Rust tests
cargo fmt --all -- --check -> PASS
cargo clippy --workspace -- -D warnings -> PASS
uv run pytest --tb=short -> PASS, 855 passed, coverage 91.69%
uv run ruff format --check -> PASS
uv run ruff check -> PASS
uv run mypy -> PASS
```

## Final Verdict

PASS — audit findings resolved.

