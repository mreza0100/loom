# Loom Build Pipeline

Run the full Loom feature pipeline for: $ARGUMENTS

All feature work goes through planning, architecture, implementation, QA, audit, and git handoff.

## Step 0 - Name The Pipeline

- Use `[Pipeline: name]` when supplied by `/wave`.
- Otherwise choose a short kebab-case name.
- `$DOCS = docs/dev/tasks/{name}`.
- `$WAVE =` supplied wave name or `none`.

Create `$DOCS` and preserve an existing `0-task.md` if the wave runner placed it.

## Step 1 - Plan

Spawn planner:

```text
Read .claude/agents/planner.md.
Mode: ANALYSIS.
Pipeline: {name}.
Feature: {request}.
Analyze crates/ and write $DOCS/1-plan.md.
Never run git commands.
```

## Step 2 - Architecture

Spawn architect:

```text
Read .claude/agents/architect.md.
Pipeline: {name}.
All pipeline docs: $DOCS/.
Write $DOCS/3-architecture.md.
Never run git commands.
```

## Step 3 - Development

Spawn developer:

```text
Read .claude/agents/developer.md.
Pipeline: {name}.
All pipeline docs: $DOCS/.
Implement in crates/.
Write $DOCS/5-dev-report.md.
Never run git commands.
```

## Step 4 - QA

Spawn QA:

```text
Read .claude/agents/qa.md.
Pipeline: {name}.
All pipeline docs: $DOCS/.
Test the Rust workspace.
Write $DOCS/6-bugs.md.
```

## Fix Loop

If `$DOCS/6-bugs.md` contains open bugs, run developer then QA again. Stop after three loops and write `$DOCS/BLOCKED.md` if still failing.

## Step 5 - Commit

Use gitter in `MERGE` phase only when commit is requested by the pipeline.

## Step 6 - Post-Commit QA

Run QA on `main` and write `$DOCS/7-post-merge-qa.md`.

## Step 7 - Audit

Run `/ca` scoped to the changed Rust code and write `$DOCS/8-audit.md`.

## Step 8 - Docs Commit

Use gitter in `DOCS-COMMIT` phase.

## Gates

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```
