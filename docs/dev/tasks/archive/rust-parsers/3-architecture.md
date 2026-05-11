> Author: architect

# Architecture - rust-parsers

## Scope

This pipeline builds Rust parser infrastructure inside `crates/loom-core` and ports the existing Python language adapters for JavaScript/TypeScript/TSX, Python, Go, Java, Rust, and C#.

Strict boundary: parser infrastructure and adapters only. No database writes, embedding calls, file walking, file watching, graph updates, MCP tool changes, or indexer pipeline orchestration. The output contract is owned `Symbol` and `ParsedEdge` values from `crates/loom-core/src/models.rs`; the future Rust indexer decides persistence and two-phase resolution.

## Current Anchors

- `Cargo.toml` is a workspace with `crates/loom-core` and `crates/loom-mcp`; workspace MSRV is Rust `1.82`.
- `crates/loom-core/src/models.rs` already defines `Symbol`, `ParsedEdge`, and `Edge`.
- `crates/loom-core/src/config.rs` already defaults watch extensions to `.py`, `.js`, `.jsx`, `.ts`, `.tsx`, `.go`, `.java`, `.rs`, `.cs`.
- `crates/loom-core/src/error.rs` has typed project errors, but no parser-specific variants yet.
- Python adapter behavior lives under `src/loom/indexer/adapters/` and is the parity source.
- High-value Python parser tests live in `tests/test_parser.py`, `tests/test_lang_adapters.py`, `tests/test_qa_lang_adapters.py`, `tests/test_qa_adapter_arch.py`, and alias-resolution tests in `tests/test_qa_foundation_data_model.py` / `tests/test_qa_bug001_recheck.py`.

## Cargo Dependencies

Add these dependencies to `crates/loom-core/Cargo.toml`:

| Crate | Version | Features | Why |
|---|---:|---|---|
| `tree-sitter` | `0.25` | `default-features = true` | Shared parser API compatible with the current first-party grammar crates selected below. |
| `tree-sitter-javascript` | `0.25` | none | JavaScript and JSX grammar. |
| `tree-sitter-typescript` | `0.23` | none | TypeScript and TSX grammars through separate `LANGUAGE_TYPESCRIPT` and `LANGUAGE_TSX` constants. |
| `tree-sitter-python` | `0.25` | none | Python and `.pyi` parsing. |
| `tree-sitter-go` | `0.25` | none | Go parsing. |
| `tree-sitter-java` | `0.23` | none | Java parsing. |
| `tree-sitter-rust` | `0.24` | none | Rust parsing. |
| `tree-sitter-c-sharp` | `0.23` | none | C# parsing. |

Do not add `bumpalo` in this pipeline. The parser result model is owned at the boundary, and tree-sitter already owns the parse tree. Add arena allocation only after profiling shows real per-file temporary allocation pressure. We are building a parser, not a shrine to premature cleverness.

Do not add `tree-sitter-language-pack`. It is promising and current, but it is too broad for this scoped pipeline, has a much younger Rust crate history, and bundles a registry problem we do not need yet.

## File Structure

Add:

```text
crates/loom-core/src/parsers/
  mod.rs
  registry.rs
  parser.rs
  tree_sitter_utils.rs
  javascript.rs
  python.rs
  go.rs
  java.rs
  rust.rs
  csharp.rs
```

Update:

```text
crates/loom-core/src/lib.rs
crates/loom-core/src/error.rs
crates/loom-core/Cargo.toml
```

Add tests:

```text
crates/loom-core/tests/parsers.rs
crates/loom-core/tests/parser_javascript.rs
crates/loom-core/tests/parser_python.rs
crates/loom-core/tests/parser_go_java_rust_csharp.rs
```

If one test file stays readable, keep fewer files. If it starts looking like an airport departure board, split it.

## Public API

Expose a stable parser surface from `crates/loom-core/src/parsers/mod.rs`:

- `ParseResult`
  - `symbols: Vec<Symbol>`
  - `edges: Vec<ParsedEdge>`
  - `Default` returns empty vectors.
- `LanguageAdapter` trait
  - `extensions() -> &'static [&'static str]`
  - `language_name() -> &'static str`
  - `excluded_dirs() -> &'static [&'static str]`
  - `parse(source: &[u8], file_path: &str) -> Result<ParseResult>`
  - `resolve_module_path(import_path: &str, source_file: &str, known_files: &BTreeSet<String>) -> String`
- `AdapterRegistry`
  - `with_builtin_adapters() -> Self`
  - `register(Box<dyn LanguageAdapter + Send + Sync>)`
  - `get_adapter(extension: &str) -> Option<&dyn LanguageAdapter>`
  - `get_all_extensions() -> BTreeSet<String>`
  - `get_all_excluded_dirs() -> BTreeSet<String>`
- `parse_file(path: &Path, source: Option<&[u8]>, registry: &AdapterRegistry) -> Result<ParseResult>`

Export `pub mod parsers;` from `crates/loom-core/src/lib.rs`. Re-export only `AdapterRegistry`, `LanguageAdapter`, `ParseResult`, and `parse_file` if tests or future indexer ergonomics benefit.

## Error Model

Add parser-specific variants to `LoomError`:

| Variant | Use |
|---|---|
| `ParserLanguage { language: String, source: tree_sitter::LanguageError }` | `Parser::set_language` failed because grammar ABI and parser crate are incompatible. |
| `ParserIo { path: String, source: std::io::Error }` | Only for `parse_file` when `source` is `None` and it reads bytes. |
| `ParserNoTree { language: String, path: String }` | `Parser::parse` returns `None`, usually timeout/cancellation or parser not initialized. |

Do not add a UTF-8 error variant for source text. Use lossy text extraction for contexts and symbol names. Source files are “UTF-8 except when they’re not,” because apparently bytes have hobbies.

Malformed source is not an error. Tree-sitter returns partial trees; adapters should extract what they can and otherwise return empty vectors.

## Registry Design

`AdapterRegistry` owns one boxed adapter per language family and a `BTreeMap<String, usize>` or `HashMap<String, usize>` from extension to adapter index.

Built-in registration order:

1. JavaScript adapter
2. Python adapter
3. Go adapter
4. Java adapter
5. Rust adapter
6. CSharp adapter

Unknown extensions return `None` from the registry and an empty `ParseResult` from `parse_file`.

Unlike Python, do not silently skip missing grammar crates. Rust grammar crates are compile-time dependencies. If one is missing, the build should fail loudly, as nature intended.

## Parser Construction

Use one `tree_sitter::Parser` per `parse` call in this pipeline. This is simpler, correct, and isolates adapter implementation from the future indexer threading model.

The future indexer pipeline can upgrade to one parser per rayon worker thread without changing adapter output. Do not share `Parser` behind a lock. `Parser::parse` needs mutable parser state; locking one global parser would serialize the hot path and then we would all pretend to be surprised by the graph.

Grammar constants in current Rust crates are `tree_sitter_language::LanguageFn` values. Adapter setup should convert them with `.into()` and pass a borrowed `tree_sitter::Language` to `Parser::set_language`.

Language selection:

| Extension | Grammar | Output language |
|---|---|---|
| `.js`, `.jsx`, `.mjs`, `.cjs` | `tree_sitter_javascript::LANGUAGE` | `javascript` |
| `.ts` | `tree_sitter_typescript::LANGUAGE_TYPESCRIPT` | `typescript` |
| `.tsx` | `tree_sitter_typescript::LANGUAGE_TSX` | `typescript` |
| `.py`, `.pyi` | `tree_sitter_python::LANGUAGE` | `python` |
| `.go` | `tree_sitter_go::LANGUAGE` | `go` |
| `.java` | `tree_sitter_java::LANGUAGE` | `java` |
| `.rs` | `tree_sitter_rust::LANGUAGE` | `rust` |
| `.cs` | `tree_sitter_c_sharp::LANGUAGE` | `csharp` |

Important correction from the Python implementation: do not feed `.ts` or `.tsx` into the JavaScript grammar. TypeScript and TSX get their dedicated grammars. Growth. Alarming, but welcome.

## Shared Utilities

`tree_sitter_utils.rs` should hold only helpers that reduce real duplication:

- `node_text<'src>(source: &'src [u8], node: Node<'_>) -> Cow<'src, str>`
  - Use `String::from_utf8_lossy` over node byte range.
  - Preserve borrowed slices when valid UTF-8.
- `node_context(source: &[u8], node: Node<'_>, max_lines: usize) -> String`
  - Match Python behavior: up to 10 source lines from node start.
- `line_start(node) -> i64` and `line_end(node) -> i64`
  - Tree-sitter rows are zero-based; Loom stores one-based lines.
- `child_by_kind(node, kind) -> Option<Node>`
- `children_by_kind(node, kind) -> Vec<Node>` or iterator helper.
- `child_by_field(node, field) -> Option<Node>`
- `walk_preorder(root, visitor)` only if it does not make adapters harder to read.
- `is_named_identifier_kind(kind)`.
- `strip_string_literal_quotes`.

Do not build an abstraction language over tree-sitter. Every language grammar has its own weird little furniture; hiding that behind generic helpers usually produces expensive fog.

## Output Semantics

Adapters return owned `Symbol` and `ParsedEdge` values.

`Symbol` fields:

- `id`: always `None`
- `name`: adapter-qualified symbol name
- `kind`: one of existing broad kinds: `function`, `class`, `method`, `variable`, `macro`
- `file`: `file_path` passed into the adapter; future indexer may rewrite to relative path
- `line`, `end_line`: one-based tree-sitter positions
- `language`: adapter language string
- `context`: up to 10 source lines

`ParsedEdge` fields:

- `source_name`: source symbol name for non-import edges; local import binding for import edges
- `target_name`: called/imported/parent/implemented symbol name
- `relationship`: existing strings: `calls`, `imports`, `extends`, `extended_by`, `implements`, `implemented_by`, `instantiates`
- `target_file`: module/path hint for imports; `None` for local symbolic edges

### Alias Fidelity

Keep the current Rust `ParsedEdge` shape for this pipeline. Do not extend `ParsedEdge` yet.

For aliased imports, preserve Python parser semantics:

- JS/TS `import { getProduct as fetchProduct } from "./product.js"` emits `source_name = "fetchProduct"`, `target_name = "getProduct"`, `target_file = "./product.js"`.
- Python pipeline later converts that into `Edge.target_name = local_name` and `Edge.original_name = exported_name` when they differ.

The Rust indexer pipeline must perform the same conversion when it arrives. Document this in parser tests with explicit assertions on `source_name` and `target_name`; do not solve downstream edge resolution here.

## Adapter Responsibilities

### JavaScript / TypeScript / TSX

Module: `javascript.rs`

Extensions:

- `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`

Excluded dirs:

- `node_modules`, `dist`, `build`, `.next`, `coverage`

Extract symbols:

- Function declarations.
- Arrow/function expressions assigned to top-level variables.
- Exported function/class/variable declarations.
- Classes.
- Methods and constructors as `Class.method` / `Class.constructor`.
- Top-level variables/constants as `variable`; skip variables inside functions.
- TypeScript type aliases and interfaces as `class` or `variable` consistently with current broad model. Prefer `class` for interfaces and `variable` for aliases.

Extract edges:

- ES imports, default imports, named imports, aliased imports.
- CommonJS `require`, including object destructuring.
- CommonJS `module.exports.X` symbols.
- `calls`, preserving full callee expression: `this.hooks.make.callAsync`, `fs.readFileSync`, `db.query`.
- Skip `console.*` calls.
- `instantiates` from `new_expression`.
- `extends` and reverse `extended_by`.
- TypeScript `implements` and interface extends where grammar nodes expose them.

Implementation notes:

- Use TSX grammar for `.tsx`, TypeScript grammar for `.ts`, JavaScript grammar for JS-family.
- Use raw node text for member/call expressions; never split to the last segment.
- Self-call guard compares full callee text with caller symbol.
- Treat parse errors as partial ASTs.

### Python

Module: `python.rs`

Extensions:

- `.py`, `.pyi`

Excluded dirs:

- `.venv`, `venv`, `.tox`, `.mypy_cache`, `.pytest_cache`

Extract symbols:

- Functions.
- Classes.
- Methods as `Class.method`, including `__init__`.
- Decorated functions/classes, including dataclass-style decorators.
- Module-level uppercase assignments as `variable`.
- Protocol-like classes are still `class`; do not invent a new kind without model work.

Extract edges:

- `import X`, `import X.Y`, aliased imports.
- `from X import Y`, relative imports, aliased from-imports.
- Skip wildcard imports.
- `extends` and reverse `extended_by` for base classes.
- Function/method body calls.
- Capitalized call names as `instantiates`.
- Attribute calls as full expression: `self.method`, `obj.method`.

Robustness:

- Empty/comment-only files return empty result.
- Malformed files return partial/empty result, not error.
- Null bytes, BOMs, and unicode identifiers/literals should not panic.

Module resolution:

- Direct match.
- Relative dotted path from `source_file`.
- Dotted absolute path to `foo/bar.py` or `foo/bar/__init__.py`.
- Return original import path on no match.

### Go

Module: `go.rs`

Extension:

- `.go`

Excluded dirs:

- `vendor`

Extract symbols:

- Package-level functions.
- Receiver methods as `Receiver.Method`, including pointer receivers.
- Structs and interfaces as `class`.
- Type aliases/definitions as `variable`.
- Consts and vars as `variable`.

Extract edges:

- Single and grouped imports.
- Struct embedded fields as `extends` and reverse `extended_by`.
- Function and selector calls as `calls`.
- Goroutine calls by handling `go_statement` and its child call expression.

Module resolution:

- Direct match.
- Tail-package matching against known files, consistent with Python `GoAdapter.resolve_module_path`.

### Java

Module: `java.rs`

Extension:

- `.java`

Excluded dirs:

- `target`, `build`, `.gradle`, `.idea`, `out`

Extract symbols:

- Classes, nested classes.
- Interfaces.
- Enums and enum constants.
- Records.
- Methods.
- Constructors as `method`.
- Fields as `variable`.
- Preserve nested qualification as `Outer.Inner.method`.

Extract edges:

- Imports and static imports.
- Skip wildcard imports.
- Class `extends` plus reverse `extended_by`.
- Class `implements`.
- Interface `extends`.
- Method invocations as `calls`, preserving qualifier when present.
- Object creation as `instantiates`.

Module resolution:

- Direct match.
- Dot path to `com/example/Foo.java`.
- Tail match on `/Foo.java`.

### Rust

Module: `rust.rs`

Extension:

- `.rs`

Excluded dirs:

- `target`

Extract symbols:

- Free functions.
- Structs as `class`.
- Enums as `class`.
- Enum variants as `Enum.Variant` variables.
- Traits as `class`.
- Trait method signatures as `Trait.method`.
- Impl methods as `Type.method`.
- Type aliases as `variable`.
- Const/static items as `variable`.
- Macro definitions as `macro`.

Extract edges:

- `use` imports, including aliases and scoped lists.
- Skip glob imports.
- Trait impls: `Struct implements Trait` and reverse `Trait implemented_by Struct`.
- Calls from function bodies.
- Method calls / field expressions as full callee text where the grammar exposes it.
- Macro invocations as `calls` to the macro name, with `!` stripped or consistently retained; choose stripped names to match symbol definitions.

Module resolution:

- Direct match.
- `crate::foo::bar` to `foo/bar.rs` or `foo/bar/mod.rs`.
- `super::foo` relative to parent directory.
- Bare `foo::bar` relative to source directory, then project-relative candidates.

### C#

Module: `csharp.rs`

Extension:

- `.cs`

Excluded dirs:

- `bin`, `obj`, `.vs`, `packages`

Extract symbols:

- Classes and partial classes as one symbol per declaration occurrence.
- Structs.
- Interfaces.
- Enums and enum members.
- Records.
- Methods.
- Constructors as `method`.
- Properties as `variable`.
- Fields as `variable`.
- Nested type qualification as `Outer.Inner.Member`.

Extract edges:

- `using` directives.
- `using static`.
- `using Alias = Type`; preserve local/exported distinction with current `ParsedEdge` fields where possible.
- Base-list entries as `extends` for base classes and `implements` for interfaces when distinguishable; if grammar context does not distinguish, prefer `extends` for parity with Python and note in test name.
- Reverse `extended_by` for extends edges.
- Invocation expressions as `calls`, preserving member access text.
- Object creation as `instantiates`.

Module resolution:

- Direct match if `import_path` exists in known files.
- Otherwise return unchanged; C# namespace resolution is mainly global symbol resolution downstream.

## Dispatcher

`parser.rs` implements `parse_file`:

- If `source` is `Some`, use those bytes.
- If `source` is `None`, read the file and map IO errors to `ParserIo`.
- Look up adapter by `path.extension()`, including leading dot to match config (`.rs`, not `rs`).
- Unknown extension returns `Ok(ParseResult::default())`.
- Delegate to adapter.

Keep max file size checks out of this layer. That belongs to file discovery/indexer selection.

## Module Resolution Contract

Each adapter owns language-specific import path resolution. The registry does not resolve modules.

Inputs are `import_path`, `source_file`, and all known files as strings. Return the resolved known file if found, otherwise return `import_path` unchanged.

Use `BTreeSet<String>` for deterministic tests. Avoid filesystem existence checks; the indexer owns the known-file universe.

## Concurrency Model

This pipeline itself is synchronous and CPU-local:

```text
caller
  -> AdapterRegistry lookup
  -> one adapter parse
  -> owned ParseResult
```

No tokio channels, no rayon pool, and no background workers are introduced here.

Thread-safety requirements for future indexer use:

- `LanguageAdapter: Send + Sync`.
- `AdapterRegistry: Send + Sync` after construction.
- Adapters must be stateless or hold only immutable static configuration.
- Parser instances are local to `parse` calls in this pipeline.

Future indexer handoff budget, documented here so the developer does not design against it:

```text
ignore::WalkParallel (num_cpus threads)
  -- bounded mpsc(32 file jobs) -->
rayon parser pool (num_cpus threads, one Parser per worker in later optimization)
  -- bounded mpsc(64 parse results) -->
embedder / writer stages owned by later pipelines
```

Tokio worker budget remains later-pipeline territory: `max(2, num_cpus / 2)` tokio workers, `num_cpus` rayon parser workers. Do not implement this in `rust-parsers`.

## Testing Plan

Required Rust tests:

- Registry:
  - All extensions registered.
  - Unknown extension returns no adapter.
  - Excluded dirs union includes adapter-specific dirs.
  - All built-in adapters report expected language names.
- Dispatcher:
  - Unknown extension returns empty result.
  - `source = Some` avoids file IO.
  - `source = None` reads a temp file.
  - Wrong extension with valid source returns empty through adapter guard.
- Cross-language robustness:
  - Empty files.
  - Comment-only files where applicable.
  - Malformed files.
  - Unicode source.
  - Null-byte source does not panic.
- JavaScript/TypeScript:
  - Named function, arrow function, function expression.
  - Exported declarations.
  - Class + methods.
  - Top-level constants/variables only.
  - Default/named/aliased imports.
  - CommonJS `require`.
  - Full call expression `this.hooks.make.callAsync`.
  - Console calls skipped.
  - `new Widget()` instantiates.
  - `.ts` and `.tsx` parse through dedicated grammars.
- Python:
  - Function/class/method/`__init__`.
  - Decorated function/class.
  - Module uppercase variable.
  - Imports/from-imports/relative import/alias/wildcard skip.
  - Extends and reverse edge.
  - Calls and instantiates.
  - `.pyi`.
- Go:
  - Function.
  - Pointer receiver method.
  - Struct/interface.
  - Const/var.
  - Grouped import.
  - Embedded struct edge.
  - Selector call and goroutine call.
- Java:
  - Class/interface/enum/record.
  - Constructor/method/field.
  - Import/static import/wildcard skip.
  - Extends/implements.
  - Nested class qualification.
  - Call and instantiation.
- Rust:
  - Function/struct/enum/variant/trait.
  - Impl method.
  - Trait method signature.
  - Trait-for-type implements and implemented_by.
  - `use` scoped list/alias/glob skip.
  - Const/static/type alias.
  - Macro definition and invocation.
- C#:
  - Class/partial class/struct/interface/enum/record.
  - Constructor/method/property/field.
  - Namespace traversal.
  - Using/static/alias.
  - Base-list edge.
  - Calls and instantiates.
- Module resolution:
  - Mirror Python tests for JS, Python, Go, Java, Rust, C# resolution behavior.

Verification commands:

```text
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
```

## Implementation Order

1. Add dependencies and parser error variants.
2. Add `parsers/mod.rs`, `ParseResult`, `LanguageAdapter`, and module exports.
3. Add registry and dispatcher.
4. Add tree-sitter utilities.
5. Implement JavaScript/TypeScript/TSX adapter and tests.
6. Implement Python adapter and tests.
7. Implement Go adapter and tests.
8. Implement Java adapter and tests.
9. Implement Rust adapter and tests.
10. Implement C# adapter and tests.
11. Add cross-language registry/dispatcher/module-resolution tests.
12. Run the required cargo verification set.

This order keeps the riskiest shared decisions visible before adapter sprawl begins. Adapter sprawl is inevitable; adapter sprawl without a harness is how software becomes an archaeological layer.

## Trade-Off Decisions

| Decision | Choice | Reason |
|---|---|---|
| Grammar sourcing | Individual first-party `tree-sitter-*` crates | Small, explicit, mature, and scoped to requested languages. |
| TypeScript parsing | Dedicated TypeScript and TSX grammars | JS grammar cannot correctly cover TS/TSX constructs. |
| Parser lifecycle | One parser per parse call for now | Correct and simple; future indexer can optimize to worker-local parsers. |
| Adapter output | Owned `Symbol` + `ParsedEdge` | Matches existing `loom-core` model and persistence boundary. |
| Alias model | Keep `ParsedEdge` unchanged | Current fields can preserve local/exported distinction until indexer conversion to `Edge.original_name`. |
| Utility abstraction | Small helper module only | Language grammars differ too much for a generic extractor to stay honest. |
| Arena allocation | Defer | No evidence yet; owned output dominates boundary costs. |
| Runtime missing grammars | Compile failure | Rust static deps should fail at build time, not hide parser holes at runtime. |

## Research Notes

### Tree-Sitter Core API

| Criteria | `tree-sitter` crate |
|---|---|
| Selected version | `0.25` |
| Current latest researched | `0.26.8` |
| crates.io downloads | 19.49M total, 6.82M recent |
| Last publish/update | 2026-03-31 |
| License | MIT |
| API needed | `Parser::new`, `Parser::set_language(&Language)`, `Parser::parse(source, None)` |
| Source | https://docs.rs/tree-sitter/latest/tree_sitter/ and https://crates.io/crates/tree-sitter |

**Decision:** Use `tree-sitter = "0.25"` for compatibility with the selected grammar versions. Current docs show grammars expose `LanguageFn` constants converted with `.into()` before `Parser::set_language`.

### Grammar Crates vs Language Pack

| Criteria | Individual `tree-sitter-*` crates | `tree-sitter-language-pack` |
|---|---:|---:|
| Scope | 7 grammar crates for requested languages | 305 languages |
| crates.io downloads | Millions per major first-party grammar | 11.15K total/recent |
| Last update | 2024-11 to 2026-04 across selected crates | 2026-05-09 |
| Maintainer/source | tree-sitter org grammar repos | kreuzberg-dev |
| Binary impact | Pay only for requested grammars | Broader registry/package surface |
| API stability | Standard grammar constants | Younger abstraction |
| License | MIT | MIT |

**Decision:** Use individual grammar crates. The language pack is useful future research, but this pipeline needs explicit, auditable parser coverage for seven requested grammar modes. Yes, boring wins. Boring ships.

### Selected Grammar Crates

| Crate | Version | Downloads | Recent Downloads | Last Update | License | Approx Crate Size |
|---|---:|---:|---:|---|---|---:|
| `tree-sitter-javascript` | `0.25.0` | 6.02M | 3.05M | 2025-09-01 | MIT | 151 KB |
| `tree-sitter-typescript` | `0.23.2` | 6.55M | 3.17M | 2024-11-11 | MIT | 829 KB |
| `tree-sitter-python` | `0.25.0` | 7.15M | 3.50M | 2025-09-11 | MIT | 180 KB |
| `tree-sitter-go` | `0.25.0` | 5.94M | 2.99M | 2025-08-29 | MIT | 110 KB |
| `tree-sitter-java` | `0.23.5` | 5.49M | 2.79M | 2024-12-21 | MIT | 160 KB |
| `tree-sitter-rust` | `0.24.2` | 9.38M | 4.26M | 2026-03-27 | MIT | 369 KB |
| `tree-sitter-c-sharp` | `0.23.5` | 2.98M | 1.55M | 2026-04-14 | MIT | 1.18 MB |

**Decision:** Pin the current compatible major/minor versions above. All are first-party or tree-sitter org grammar crates, MIT licensed, and comfortably above the Rust dependency threshold in the architect manual.

### Grammar API Details

| Language | Constant(s) | Source |
|---|---|---|
| JavaScript/JSX | `tree_sitter_javascript::LANGUAGE` | https://docs.rs/tree-sitter-javascript/latest/tree_sitter_javascript/ |
| TypeScript/TSX | `LANGUAGE_TYPESCRIPT`, `LANGUAGE_TSX` | https://docs.rs/tree-sitter-typescript/latest/tree_sitter_typescript/ |
| Python | `tree_sitter_python::LANGUAGE` | https://docs.rs/tree-sitter-python/latest/tree_sitter_python/ |
| Go | `tree_sitter_go::LANGUAGE` | https://docs.rs/tree-sitter-go/latest/tree_sitter_go/ |
| Java | `tree_sitter_java::LANGUAGE` | https://docs.rs/tree-sitter-java/latest/tree_sitter_java/ |
| Rust | `tree_sitter_rust::LANGUAGE` | https://docs.rs/tree-sitter-rust/latest/tree_sitter_rust/ |
| C# | `tree_sitter_c_sharp::LANGUAGE` | https://docs.rs/tree-sitter-c-sharp/latest/tree_sitter_c_sharp/ |

The current docs show the same usage pattern: create `Parser`, convert the grammar constant with `.into()`, call `set_language`, then `parse`.

## Non-Goals

- No new languages.
- No Tree-sitter query DSL migration.
- No semantic embeddings.
- No SQLite writes.
- No graph coupling.
- No file discovery or watcher.
- No MCP tool changes.
- No `ParsedEdge` schema change unless implementation proves current alias preservation is impossible.

## Developer Hand-Off Checklist

- Parser modules compile under workspace Rust `1.82`.
- All adapters are registered by extension.
- Unknown extensions and malformed files are non-fatal.
- TS and TSX use TypeScript grammars.
- Full JS/TS call expressions are preserved.
- Alias import local/exported distinction survives in `ParsedEdge`.
- Adapter tests mirror the Python behavior that matters.
- Required cargo verification commands pass.
