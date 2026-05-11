# Pipeline: corepack-head-to-head-run

Wave: `semantic-proof-gate`

## Final Required Run

Run the Corepack task once with grep/no-MCP only, wait for it to finish completely, then run the exact same task once with the new Loom only.

## Constraints

- Use fresh Corepack clones at the same commit.
- Use the same model.
- Use the same prompt except tool availability.
- Use isolated configs.
- Execute sequentially only.

## Required Metrics

Measure every available metric:

- Start/end timestamps.
- Wall time.
- Exit status.
- Model.
- Sandbox.
- Prompt hash.
- Repo commit.
- Setup time/failures.
- Index build time.
- Index size.
- Vector backend.
- Embedder backend/status/fingerprint.
- MCP server startup time.
- Tool-call count.
- Per-tool latency.
- Per-tool request/response chars.
- MCP calls by tool.
- Shell calls by command.
- Shell output chars.
- File-read count.
- Input/output/reasoning/cached tokens when available.
- Total event count.
- Final answer chars.
- Cited files/lines.
- Required evidence coverage.
- Exact-hit useful symbols.
- Beyond-grep useful symbols.
- Useful symbols per token.
- Shell escape rate.
- Shell escape cause labels.
- Missing evidence.
- Answer-quality checklist.
- Diff from expected evidence set.
- Regression verdict.

## Output

Produce a final report under `tmp/benchmark/corepack-gate/` with tables, artifacts, diagnosis, and a clear verdict:

- `LOOM_BEATS_GREP`
- `LOOM_TIES_GREP`
- `LOOM_FAILS_GATE`

