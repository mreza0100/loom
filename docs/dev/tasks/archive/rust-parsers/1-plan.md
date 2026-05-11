> Author: planner

# Plan — rust-parsers

## Feature Context
Implement Rust tree-sitter parser infrastructure and language adapters for the Rust rewrite so Loom can produce the same `Symbol` + `ParsedEdge` parser output as the Python indexer before the Rust indexer pipeline consumes it.

## Current State
- Rust foundation exists in `Cargo.toml`, `crates/loom-core`, and `crates/loom-mcp`.
- `crates/loom-core/src/models.rs` already defines owned `Symbol`, `ParsedEdge`, and `Edge` structs. The current parser layer should return these owned persistence-boundary types, while using borrowed helper views internally where useful. Yes, ownership doing its little safety dance. Cute.
- `crates/loom-core/src/config.rs` already includes parser-relevant defaults:
  - `watch_extensions`: `.py`, `.js`, `.jsx`, `.ts`, `.tsx`, `.go`, `.java`, `.rs`, `.cs`
  - `excluded_dirs`: `.git`, `__pycache__`, `.loom`, `node_modules`, `.venv`, `venv`, `target`, `dist`, `build`
  - `max_file_size_bytes`
- `crates/loom-core/src/store/mod.rs` already stores parser output through `insert_symbol`, `insert_symbols`, `insert_edge`, and `insert_edges`; it also carries unresolved edges via `target_name`, `target_file`, and `original_name`.
- `crates/loom-core/src/error.rs` has no parser-specific error variants yet. Adapters can initially return empty output for unknown extensions and partial ASTs, but parser setup errors need explicit typed errors rather than vibes.
- No Rust parser infrastructure currently exists. There is no `crates/loom-core/src/parsers/` module and no tree-sitter dependencies in `crates/loom-core/Cargo.toml`.
- Python parser infrastructure to port:
  - `src/loom/indexer/adapters/base.py`: `LanguageAdapter` protocol and `AdapterRegistry`
  - `src/loom/indexer/adapters/__init__.py`: singleton registry and extension lookup
  - `src/loom/indexer/parser.py`: file-extension dispatcher
  - `src/loom/indexer/pipeline.py`: consumer contract for parser output and import resolution
- Python adapters already cover the requested languages:
  - `src/loom/indexer/adapters/javascript.py`: JS/TS/TSX/JSCJS/MJS, functions, classes, variables/constants, imports, requires, calls, instantiates, extends/implements, full call expressions
  - `src/loom/indexer/adapters/python.py`: `.py`/`.pyi`, imports, relative import resolution, decorated definitions, functions, classes, methods, uppercase module constants, calls, instantiates, extends
  - `src/loom/indexer/adapters/go.py`: imports, functions, receiver methods, structs, interfaces, const/var/type declarations, embedding/extends, calls
  - `src/loom/indexer/adapters/java.py`: imports, classes, interfaces, enums, records, methods, constructors, fields, extends/implements, calls, instantiates
  - `src/loom/indexer/adapters/rust.py`: `use`, functions, signatures, structs, enums, traits, impl blocks, type aliases, const/static, macro definitions, macro/call extraction, trait implementation edges
  - `src/loom/indexer/adapters/csharp.py`: using directives, namespaces, classes, structs, interfaces, enums, records, methods, constructors, properties, fields, base-list edges, calls, instantiates
- Python tests to mirror into Rust live in `tests/test_parser.py`, `tests/test_lang_adapters.py`, and `tests/test_qa_lang_adapters.py`. They are the behavioral contract, not decorative wallpaper.

## Gaps & Needed Changes
- Add parser dependencies to `crates/loom-core/Cargo.toml`:
  - `tree-sitter`
  - `tree-sitter-javascript`
  - `tree-sitter-typescript`
  - `tree-sitter-python`
  - `tree-sitter-go`
  - `tree-sitter-java`
  - `tree-sitter-rust`
  - `tree-sitter-c-sharp`
  - optional helper dependency only if implementation proves it pays for itself: `once_cell` or `std::sync::OnceLock` for shared language handles, and possibly `bumpalo` only for real per-file temporary allocation pressure.
- Add parser exports to `crates/loom-core/src/lib.rs`:
  - `pub mod parsers;`
  - Re-export only stable entry points if useful: `AdapterRegistry`, `LanguageAdapter`, `ParseResult`, `parse_file`.
- Add `crates/loom-core/src/parsers/mod.rs`:
  - Define `ParseResult { symbols: Vec<Symbol>, edges: Vec<ParsedEdge> }`.
  - Define `LanguageAdapter` trait:
    - `fn extensions(&self) -> &'static [&'static str]`
    - `fn language_name(&self) -> &'static str`
    - `fn excluded_dirs(&self) -> &'static [&'static str]`
    - `fn parse(&self, source: &[u8], file_path: &str) -> Result<ParseResult>`
    - `fn resolve_module_path(&self, import_path: &str, source_file: &str, known_files: &BTreeSet<String>) -> String`
  - Use `Result<ParseResult>` for setup/internal failures, while malformed source returns partial/empty results from tree-sitter.
- Add `crates/loom-core/src/parsers/registry.rs`:
  - Implement `AdapterRegistry` keyed by extension.
  - Register adapters once through `AdapterRegistry::default()` or `AdapterRegistry::with_builtin_adapters()`.
  - Provide `get_adapter`, `get_all_extensions`, and `get_all_excluded_dirs`.
  - Avoid Python-style dynamic import skipping. Rust grammar crates are compile-time dependencies, so missing grammar should be a compile error, not a charming runtime shrug.
- Add `crates/loom-core/src/parsers/parser.rs`:
  - Implement `parse_file(path: &Path, source: Option<&[u8]>, registry: &AdapterRegistry) -> Result<ParseResult>`.
  - Return empty `ParseResult` for unknown extensions.
  - Keep file reading out of adapters; adapters accept bytes.
- Add shared helper module `crates/loom-core/src/parsers/tree_sitter_utils.rs`:
  - `node_text(source, node) -> Cow<'src, str>` using `String::from_utf8_lossy`.
  - `context(source, node, max_lines) -> String`.
  - child lookup helpers by type/name field.
  - line conversion from tree-sitter zero-based points to Loom one-based `line`/`end_line`.
  - recursive traversal utilities where they reduce duplication without becoming a second language nobody asked for.
- Add one module per adapter:
  - `crates/loom-core/src/parsers/javascript.rs`
  - `crates/loom-core/src/parsers/python.rs`
  - `crates/loom-core/src/parsers/go.rs`
  - `crates/loom-core/src/parsers/java.rs`
  - `crates/loom-core/src/parsers/rust.rs`
  - `crates/loom-core/src/parsers/csharp.rs`
- JavaScript/TypeScript adapter specifics:
  - Register `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`.
  - Use JavaScript grammar for JS-family files and TypeScript/TSX grammars for `.ts`/`.tsx`; do not feed TSX to the JS grammar and call it innovation.
  - Preserve full call expressions such as `this.hooks.make.callAsync`.
  - Support ES imports, CommonJS `require`, arrow/function expressions, exported declarations, classes, methods, constructors, uppercase constants, instantiation edges, and extends/implements edges.
  - Preserve import alias information where possible by adding `original_name` later when pipeline converts `ParsedEdge` to `Edge`; if `ParsedEdge` remains too small, plan a follow-up model extension before the indexer pipeline ports alias handling.
- Python adapter specifics:
  - Register `.py` and `.pyi`.
  - Handle decorated definitions, dataclass-like decorated classes/functions, class methods including `__init__`, nested class behavior consistent with current tests, module uppercase constants, imports, from-imports, aliased imports, wildcard skip, relative imports, calls, instantiates, and multiple inheritance.
  - Must tolerate malformed partial ASTs, null bytes, BOMs, unicode identifiers/literals, and empty/comment-only files.
- Go adapter specifics:
  - Register `.go`.
  - Extract package-level functions, receiver methods named `Receiver.method`, structs, interfaces, type aliases/definitions, consts, vars, imports, embedded types as `extends`, selector/function calls, and goroutine calls by inspecting `go_statement` / call children.
  - Resolve package paths by direct match and tail-file/package matching, consistent with Python `GoAdapter.resolve_module_path`.
- Java adapter specifics:
  - Register `.java`.
  - Extract package imports, static imports, classes, nested classes, interfaces, enums and enum constants, records, constructors, methods, fields, annotations where represented in context, generics without crashing, extends/implements/interface-extends, calls, and instantiations.
  - Skip wildcard imports rather than emitting `*`.
- Rust adapter specifics:
  - Register `.rs`.
  - Extract `use` statements including aliases and scoped lists, functions, trait method signatures, structs, enums and variants, traits, impl methods, trait-for-type implementation edges, type aliases, const/static items, macro definitions, macro invocations, calls, and method calls.
  - Resolve `crate::`, `super::`, bare module paths, `mod.rs`, and sibling module files.
- C# adapter specifics:
  - Register `.cs`.
  - Extract namespaces, `using` including alias/static, classes, partial classes, structs, interfaces, enums, records, constructors, methods, properties, fields, base-list extends/implements/interface-extends, calls, and instantiations.
  - Preserve nested type qualification and namespace context where current Python behavior expects it.
- Add parser-specific error variants to `crates/loom-core/src/error.rs` if needed:
  - `ParserLanguage(String)`
  - `ParserIo { path, source }` only if `parse_file` owns reads
  - `ParserUtf8(String)` should usually be avoided by using lossy text extraction for robustness.
- Add Rust tests:
  - `crates/loom-core/tests/parsers.rs` for registry, dispatcher, unknown extensions, empty/malformed files, and cross-language smoke tests.
  - Per-language focused fixtures either in the same file or under `crates/loom-core/tests/parsers/` if the test file gets bloated.
  - Mirror the high-value assertions from `tests/test_parser.py`, `tests/test_lang_adapters.py`, and `tests/test_qa_lang_adapters.py`: malformed source, nested declarations, imports, aliases, representative calls, full JS call expressions, `.tsx`, `.pyi`, Go receivers, Java/C# constructors/properties, Rust impl/trait/use/macro handling.

## Integration Surface
- Public parser API for the future Rust indexer:
  - `AdapterRegistry::with_builtin_adapters()`
  - `AdapterRegistry::get_adapter(extension)`
  - `AdapterRegistry::get_all_extensions()`
  - `AdapterRegistry::get_all_excluded_dirs()`
  - `parse_file(path, source, registry) -> Result<ParseResult>`
- Adapter API must remain independent of storage and embeddings:
  - Input: source bytes + file path string.
  - Output: `Vec<Symbol>` with `file`, `line`, `end_line`, `language`, `kind`, `context`, and `id: None`.
  - Output: `Vec<ParsedEdge>` with parser-local `source_name`, `target_name`, `relationship`, and optional `target_file`.
- Store integration remains through existing `LoomDb` methods:
  - The indexer pipeline will set relative `Symbol.file`, insert symbols, map `ParsedEdge.source_name` to inserted source IDs, and insert unresolved `Edge` rows.
  - Import edges must continue to provide enough information for future two-phase resolution in Rust. Current `ParsedEdge` lacks `original_name`, so alias fidelity needs either a model extension or careful encoding before the Rust pipeline ports alias-aware resolution.
- Config integration:
  - `LoomConfig.watch_extensions` should be reconciled with `AdapterRegistry::get_all_extensions()` when the Rust indexer is added.
  - `LoomConfig.excluded_dirs` should union registry adapter exclusions, matching Python behavior currently tested through config propagation.
  - `max_file_size_bytes` belongs in the indexer file-selection layer, not inside adapters.
- Threading integration:
  - `tree_sitter::Parser::parse` requires mutable parser state. The implementation should keep parser instances local to each adapter parse call initially, then let the indexer pipeline upgrade to one parser per rayon worker if profiling demands it.
  - Shared `tree_sitter::Language` values should be static/cloneable handles. Do not share `Parser` behind locks; that is how performance goes to sit in the corner and think about what it did.
- MCP surface:
  - No MCP tool signatures change in this pipeline.
  - `search`, `related`, `impact`, `neighborhood`, `reindex`, and `status` will benefit indirectly once the Rust indexer uses parser output.

## Risks & Dependencies
- Depends on `rust-foundation` being present, which it is: workspace, core models, config, store, and graph exist.
- Tree-sitter crate APIs differ by grammar crate version. The developer must pin compatible grammar versions and verify language constructor names before implementation.
- TypeScript and TSX need dedicated grammar functions. Current Python `JavaScriptAdapter` uses the JavaScript grammar for `.ts` and `.tsx`; the Rust rewrite should fix that rather than faithfully porting a questionable habit. Growth, apparently.
- `ParsedEdge` may be too small for alias-preserving import edges. Python stores `original_name` only after pipeline conversion to `Edge`; Rust parser work should either keep parity within current model constraints or explicitly extend the model before downstream indexer work starts.
- Line/byte handling must be consistent. Tree-sitter uses byte offsets and zero-based points; Loom stores one-based lines. Unicode and lossy decode cases need tests because source code loves being “mostly UTF-8” at the worst possible moment.
- Full language parity is broad. Implementation order should be:
  1. Parser trait, registry, dispatcher, shared helpers.
  2. JavaScript/TypeScript adapter and tests.
  3. Python adapter and tests.
  4. Go, Java, Rust, C# adapters and tests.
  5. Cross-adapter registry/config tests and full cargo verification.
- Keep parser modules pure. No database writes, embedding calls, file walking, watcher behavior, or MCP server wiring in this pipeline.

## Research Needed
- Confirm exact current crate names, versions, and API signatures for:
  - `tree-sitter`
  - `tree-sitter-javascript`
  - `tree-sitter-typescript`
  - `tree-sitter-python`
  - `tree-sitter-go`
  - `tree-sitter-java`
  - `tree-sitter-rust`
  - `tree-sitter-c-sharp`
- Confirm whether `tree-sitter-c-sharp` exposes a Rust crate/version compatible with the selected `tree-sitter` version; C# grammar crates sometimes enjoy making package naming a personality trait.
- Check whether `tree_sitter::Language` in the pinned version is `Copy`/`Clone` and how grammar crates expose language handles.
- Verify TypeScript and TSX grammar selection with fixtures before writing the full JS/TS adapter.
- Decide whether alias fidelity requires extending `ParsedEdge` now or deferring to the Rust indexer pipeline task. If extending now, update serialization tests in `crates/loom-core/tests/foundation.rs`.
- Benchmark parser allocation only after correctness lands. `Cow<'src, str>` in helpers is useful now; `bumpalo` should wait for evidence.
