# Wave 2 — Full Rust Rewrite

This archived wave spec was sourced from repo-root `wave.md` on 2026-05-11.

See repo-root `wave.md` for the complete locked source spec. Pipeline-specific task subsets are pre-placed in:

- `docs/dev/tasks/rust-foundation/0-task.md`
- `docs/dev/tasks/rust-parsers/0-task.md`
- `docs/dev/tasks/rust-indexer/0-task.md`
- `docs/dev/tasks/rust-productization/0-task.md`

## Grouping Decision

The Rust rewrite is split into four sequential dependency stages:

1. `rust-foundation`: Tasks 1-3.
2. `rust-parsers`: Tasks 4-5.
3. `rust-indexer`: Tasks 6-8.
4. `rust-productization`: Tasks 9-11.

This grouping preserves the real dependency chain while still grouping aggressively inside each subsystem.

