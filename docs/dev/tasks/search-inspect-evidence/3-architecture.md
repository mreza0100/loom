# Architecture - search-inspect-evidence

## Goals

- Preserve exact/beyond search buckets while making default search-family responses compact and handle-first.
- Make source inspection an explicit, bounded, read-only workflow.
- Add an evidence-pack tool that gathers proof and citations without writing the final answer for the caller.
- Teach MCP clients the intended containment sequence through descriptions, schemas, annotations, and actionable errors.

## File Responsibilities

- `crates/loom-core/src/models.rs`
  - Own compact `FileAnchor`, `ResponseBudget`, `InspectSnippet`, `InspectResponse`, and `EvidencePackResponse` structs.
  - Add reversible file handles and compact hit metadata.
- `crates/loom-core/src/search/engine.rs`
  - Populate compact hit fields for search, related, impact, and neighborhood responses.
  - Resolve symbol/file handles, reject stale revisions, read bounded snippets under `target_dir`, and build evidence packs.
- `crates/loom-core/src/store/mod.rs`
  - Continue to provide symbol lookup and index revision facts; no schema migration is required.
- `crates/loom-mcp/src/server.rs`
  - Register `inspect` and `evidence_pack`.
  - Validate budgets and handle strings.
  - Advertise read-only containment and improved retry guidance.
- `docs/dev/runtime-contract.md`
  - Document the new compact default response and inspect/evidence workflows.
- Tests
  - Core tests cover stale handles, budgeted snippets, file handles, evidence pack content, and compact JSON shape.
  - MCP tests cover tool registration, descriptions, read-only hints, and validation errors.

## Data Model / API Changes

- Search-family hits expose:
  - `handle`
  - `file_handle`
  - `rank`
  - `name`
  - `kind`
  - `language`
  - `anchor`
  - `summary`
  - `score`
  - `reason_codes`
  - optional bounded lexical evidence
  - compact coupled handles
- Internal `symbol` payloads remain available to Rust callers but are skipped during JSON serialization so MCP defaults do not leak source-like context.
- File handles use `file:{index_revision}:{hex(repo_relative_path)}` so they are stable and reversible within one index revision.
- `inspect` returns a versioned object with stale status, citable anchor, snippet text, line range, page metadata, budget metadata, and actionable error text.
- `evidence_pack` returns exact hits, beyond-grep hits, inspected snippets, coverage checklist, omitted/truncated metadata, and missing concepts within the caller budget.

## Algorithms

- Compact hit construction derives anchors from indexed file/line spans and summaries from the first non-empty line of symbol context.
- Symbol handle resolution checks the embedded revision against the current `index_revision`, then resolves the numeric symbol id.
- File handle resolution checks the revision, decodes the repo-relative path, confirms it is indexed, and reads only through a contained path under `target_dir`.
- Inspection computes a start/end line window, applies line and character budgets, and emits pagination metadata instead of reading or returning a whole large file.
- Evidence pack runs search with a budget-derived limit, selects top exact and beyond-grep handles, inspects small snippets, records omitted items, and leaves missing concept detection as explicit metadata when no exact/beyond evidence exists.

## Test Plan

- `cargo test -p loom-core --test search`
  - compact hit metadata and JSON shape
  - symbol inspect success
  - stale handle response
  - file handle pagination and budget truncation
  - evidence pack exact/beyond/snippet/checklist output
- `cargo test -p loom-core --test foundation`
  - updated JSON shape and handle helper compatibility
- `cargo test -p loom-mcp`
  - tool registration includes `inspect` and `evidence_pack`
  - read-only annotations and action-oriented descriptions
  - validation rejects empty handles and invalid budgets
- Workspace gates when feasible:
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo fmt --all -- --check`

## Risks

- This pipeline intentionally changes the public JSON shape again; downstream tools must use handles and anchors instead of full symbols.
- Current lexical coverage is still symbol-FTS based, not full-file grep equivalence. Runtime docs must continue to avoid claiming complete grep replacement.
- Evidence pack quality will improve in later role-card and state-flow waves; this pipeline should expose omissions instead of pretending those signals already exist.
