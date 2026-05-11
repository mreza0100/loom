# Pipeline: rust-runtime-authority

Wave: `semantic-proof-gate`

## Task 2 - Rust authority and migration cleanup

Declare the Rust runtime as the only authoritative active implementation and remove stale Python runtime assumptions from active docs, benchmark scripts, configs, and agent manuals.

## Required Behaviors

- Active benchmark configs invoke Rust `loom-mcp`.
- Active scripts use `.loom/loom.db`.
- Historical Python/Rust comparison docs are archived or labeled as historical research.
- A guard fails if active benchmark configs reference `python -m loom`.
- Docs define Rust tool JSON, status fields, scoring semantics, storage path, schema/version, and benchmark metric contracts.

## Boundaries

- Do not preserve retired runtime compatibility unless explicitly marked historical.
- Preserve unrelated existing worktree changes.

## Verification

- Add or update tests/guards that fail on active `python -m loom` benchmark references.
- Run docs/config checks plus workspace gates when feasible.

