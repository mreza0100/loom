# Post-Merge QA — rust-parsers

## Result

PASS.

## Rust Gates

```text
cargo build --workspace -> PASS
cargo test --workspace -> PASS, 26 tests
cargo fmt --all -- --check -> PASS
cargo clippy --workspace -- -D warnings -> PASS
```

Rust test breakdown:

- `foundation.rs`: 12 passed
- `parsers.rs`: 6 passed
- `qa_rust_foundation.rs`: 3 passed
- `test_qa_rust_parsers.rs`: 5 passed

## Python Gates

```text
uv run pytest --tb=short -> PASS, 855 passed, coverage 91.69%
uv run ruff format --check -> PASS
uv run ruff check -> PASS
uv run mypy -> PASS, 25 source files
```

## Coverage Checked

- Malformed/null-byte parser inputs across Rust, TSX, Python, Go, Java, and C#.
- TSX grammar path for JSX plus generic arrow syntax.
- CommonJS destructured `require` alias preservation.
- Rust scoped `use` list prefix and alias preservation.
- Empty/comment-only parser behavior.
- Registry/dispatcher/parser smoke coverage.

## Notes

- No push performed.
- Pytest still reports the existing unknown `asyncio_mode` warning; non-blocking.

