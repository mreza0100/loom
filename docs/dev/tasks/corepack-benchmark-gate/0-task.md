# Pipeline: corepack-benchmark-gate

Wave: `semantic-proof-gate`

## Tasks

### Task 18 - Deterministic benchmark harness for grep, Octocode, and Loom versions

Turn ad hoc benchmark machinery into a repeatable harness that runs fresh clones, fixed prompts, isolated MCP configs, event parsing, and comparable reports across:

- grep/no-MCP.
- Octocode.
- current Loom.
- future Loom variants.

Required recorded data:
- Wall time.
- MCP calls.
- Shell calls.
- Input/output/reasoning/cached tokens when available.
- MCP chars.
- Shell chars.
- Setup failures.
- Repo commit.
- Prompt hash.
- Model.
- Sandbox.
- Tool timeouts.
- Index fingerprint.
- Final-answer quality checklist.

Artifacts must live under `tmp/`; reports must include table and verdict.

### Task 19 - Shell-escape and useful-symbol metrics

Add first-class benchmark metrics for the north star:

- Shell escape rate.
- Shell output chars.
- Useful symbols discovered per token.
- Beyond-grep useful hits.
- Evidence completeness.

Every benchmark task must define required files/functions/facts/relationships, aliases, source spans when known, exact-hit vs beyond-grep usefulness, evidence completeness, and shell-escape attribution:

- `missing_evidence`.
- `missing_exact_lines`.
- `missing_confidence`.
- `verification_only`.
- `model_habit`.
- `setup_failure`.

### Task 20 - Corepack containment acceptance gate

Create an executable acceptance benchmark for the package-manager-shim Corepack task that Loom must pass before claiming "beats grep".

Required gate:
- Runs on fresh Corepack clones at the same commit.
- Requires answer quality equal or better than grep/no-MCP.
- Requires lower shell output.
- Requires lower or comparable input tokens.
- Requires visible beyond-grep value.

Expected evidence must include:
- Shim generation.
- `runMain`.
- `executePackageManagerRequest`.
- `findProjectSpec`.
- `parsePackageJSON`.
- `devEngines`.
- Env vars.
- Transparent commands.
- Defaults.
- Install.
- `runVersion`.

Failure produces a diagnosis table.

## Verification

- Run deterministic benchmark harness tests.
- Run the Corepack gate dry-run or fixture mode before the final live gate.

