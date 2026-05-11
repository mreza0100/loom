# Audit - semantic-embedder-authority

No findings.

## Scope

- `crates/loom-core/src/embedder.rs`
- `crates/loom-core/src/jina_bert.rs`
- `crates/loom-core/src/lib.rs`
- `crates/loom-core/tests/embedder.rs`
- `crates/loom-mcp/src/main.rs`
- `crates/loom-mcp/src/server.rs`

## Checks

- The Jina model implementation loads only local cache paths returned by the configured `ModelSource`.
- No indexed source content is logged.
- Candle failures remain actionable `EmbedderModel` errors unless explicit hashing fallback is configured.
- Hashing fallback is still opt-in through config and reports degraded status when used after Candle failure.
- Status exposes backend, degraded state, model, and dimensions without forcing model load.
- Dynamic ALiBi avoids the previous full-context startup allocation.
- Payload changes are structured JSON fields.

## Verification

- `cargo clippy --workspace -- -D warnings` - PASS
- `cargo fmt --all -- --check` - PASS
- `cargo test --workspace` - PASS

Residual risk: live Jina smoke is intentionally environment-gated because first-run model download is network-sensitive.
