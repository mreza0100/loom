# Competitor Dominance Wave

Source task file: `wave.md`

Pipeline:

- `competitor-dominance-cli` - CLI parity, fact/callsite proof handles, benchmark helper parity, BM/RND gate.

Rationale:

- The live runtime already contains the competitor report's highest-risk retrieval primitives: exact/beyond buckets, inspect handles, evidence packs, behavior facts, callsites, role cards, and containment-oriented MCP descriptions.
- The next best call is to expose the same intelligence through a CLI, make operational facts/callsites inspectable, and make benchmark helpers exercise the active Rust product surface.
- This keeps the wave small enough to verify before expensive BM/RND runs.
