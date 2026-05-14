# Loom Super Upgrade Wave

Source task file: `wave.md`

Pipeline:

- `super-upgrade-retrieval` - exact symbol enumeration, compact expansion/inspection, BM metric gate.

Rationale:

- RND3 beat grep on useful symbols per noncached+output token, but grep still won most raw cost/time/noise metrics.
- The next product primitive is exact, compact enumeration for cases like command `execute` methods.
- Expansion and inspection tools need stricter output budgets so Loom agents stop buying huge MCP payloads.
- The BM gate must answer the user's explicit bar: more than 60% of comparable metrics better than grep.
