> Author: wave

# Task: rust-parsers

Wave: rust-rewrite

Source task file: `wave.md`

## Scope

Implement Rust tree-sitter language adapter infrastructure and adapters for JavaScript, TypeScript, Python, Go, Java, Rust, and C#.

## Included Wave Tasks

### 4. Tree-sitter parsers: JS/TS + Python adapters

Port `LanguageAdapter` and `AdapterRegistry` from Python to Rust:
- trait-based adapter interface
- registry keyed by extension
- one parser instance per worker thread
- `Language` shared safely
- `Cow<'src, str>` where source borrowing is possible, owned strings at persistence boundaries

Implement JavaScript, TypeScript, TSX, and Python adapters using tree-sitter grammar crates. Extract:
- functions/classes/methods
- constants
- type aliases/interfaces/protocols
- imports
- calls
- extends/implements
- instantiates/type references

Preserve full call expressions such as `this.hooks.make.callAsync`.

Python requirements: decorators, dataclasses, `__init__`, imports, classes/functions/methods, malformed partial ASTs.

### 5. Tree-sitter parsers: Go, Java, Rust, C# adapters

Implement adapters for:
- Go: functions, methods with receiver types, structs, interfaces, goroutine calls.
- Java: classes, methods, interfaces, annotations, generics, package imports.
- Rust: functions, structs, enums, traits, impl blocks, `use` statements, macro invocations.
- C#: classes, methods, interfaces, properties, namespaces, `using` statements, partial classes.

All adapters return the same `Symbol` + `ParsedEdge` types and register through the shared registry.

## Dependencies

Depends on `rust-foundation` for workspace, shared types, and error/config structure.

## Required Verification

- `cargo build --workspace`
- `cargo test --workspace`
- parser tests for malformed files, empty files, nested declarations, imports, and representative calls
- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`

