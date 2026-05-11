# Dev Report — rust-parsers

## Implementation Summary

Implemented the scoped Rust parser infrastructure in `crates/loom-core`:

- Added tree-sitter grammar dependencies for JavaScript, TypeScript/TSX, Python, Go, Java, Rust, and C#.
- Added parser-specific `LoomError` variants for grammar setup, parser IO, and no-tree failures.
- Added `parsers` module with:
  - `ParseResult`
  - `LanguageAdapter`
  - `AdapterRegistry`
  - `parse_file`
  - tree-sitter helper utilities
- Added adapters for:
  - JavaScript/TypeScript/TSX
  - Python
  - Go
  - Java
  - Rust
  - C#
- Exported parser entry points from `loom-core`.

Boundary check: no storage, indexer, graph, or MCP behavior was changed beyond the required core exports, errors, and dependencies. The parsers return owned `Symbol` and `ParsedEdge` values only. Alias import fidelity stays inside the current `ParsedEdge` shape by preserving local/exported names as `source_name` and `target_name`.

Fix loop iteration 1:

- Fixed `BUG-RUST-PARSERS-001` in `crates/loom-core/src/parsers/javascript.rs`: destructured CommonJS `require` now emits one import edge per binding and preserves aliases as local/exported `source_name`/`target_name` pairs.
- Fixed `BUG-RUST-PARSERS-002` in `crates/loom-core/src/parsers/rust.rs`: scoped Rust `use` lists now carry the prefix into each list member and alias, while avoiding intermediate prefix-only import edges.
- Parser scope stayed pure: no storage, embedding, watcher, graph, MCP, or indexer orchestration changes.

## Test Coverage

Added focused Rust parser tests in `crates/loom-core/tests/parsers.rs` covering:

- registry extension coverage
- excluded directory union
- unknown extension behavior
- dispatcher source injection and disk reads
- JavaScript/TypeScript symbols, imports, alias imports, full call expressions, instantiation, inheritance
- Python decorated/class/method parsing, imports, relative imports, constants, malformed partial input, calls, instantiation
- Go functions, receiver methods, structs, imports, calls
- Java classes, nested classes, methods, fields, imports, inheritance, calls, instantiation
- Rust use imports, scoped use-list aliases, consts, structs, enums/variants, traits, impl methods, trait implementations, macros, calls
- C# usings, aliases, partial classes, properties, fields, inheritance, calls, instantiation
- adapter module resolution behavior

Added QA regression coverage in `crates/loom-core/tests/test_qa_rust_parsers.rs` covering malformed parser tolerance, TSX grammar selection, destructured CommonJS `require` import edges, scoped Rust `use` list import edges, and empty/comment-only source behavior.

Rust test result: `cargo test --workspace` passed with 26 tests:

- `foundation.rs`: 12 passed
- `parsers.rs`: 6 passed
- `qa_rust_foundation.rs`: 3 passed
- `test_qa_rust_parsers.rs`: 5 passed
- doc tests / crate unit tests: 0 tests, passed

Python coverage from `uv run pytest`: 855 passed, 1 warning, total coverage 91.69%.

## Runbook

Commands run:

```text
cargo fmt --all
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
cargo test --test test_qa_rust_parsers -- --nocapture
uv run pytest
uv run ruff check
uv run mypy
uv run mypy src
uv run ruff format --check
```

Exact outcomes:

```text
cargo fmt --all
PASS

cargo build --workspace
PASS

cargo test --workspace
PASS, 26 Rust tests passed

cargo clippy --workspace -- -D warnings
PASS

cargo fmt --all -- --check
PASS

cargo test --test test_qa_rust_parsers -- --nocapture
PASS, 5 passed

uv run pytest
PASS, 855 passed, 1 warning, 91.69% coverage

uv run ruff check
PASS

uv run mypy
FAIL, mypy reported: Missing target module, package, files, or command.

uv run mypy src
PASS, Success: no issues found in 25 source files

uv run ruff format --check
PASS, 46 files already formatted
```

No new environment variables are required.
