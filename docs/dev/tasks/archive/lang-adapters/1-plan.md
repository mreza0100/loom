> Author: planner

# Plan — lang-adapters

## Feature Context

Implement five language adapters (Python, Go, Java, Rust, C#) conforming to the `LanguageAdapter` Protocol established by adapter-arch, and update the registry `__init__.py` to register all six adapters (including the existing `JavaScriptAdapter`).

---

## Current State

### Relevant files

- `/Users/reza/work/loom/src/loom/indexer/adapters/base.py` — `LanguageAdapter` Protocol (structural subtyping, `@runtime_checkable`) and `AdapterRegistry`. Three required class-level attributes (`extensions`, `language_name`, `excluded_dirs`) and two methods (`parse`, `resolve_module_path`).
- `/Users/reza/work/loom/src/loom/indexer/adapters/__init__.py` — REGISTRY singleton. Currently imports and registers only `JavaScriptAdapter`.
- `/Users/reza/work/loom/src/loom/indexer/adapters/javascript.py` — Reference implementation. All new adapters must follow this exact structural pattern.
- `/Users/reza/work/loom/src/loom/indexer/parser.py` — Thin dispatcher; calls `REGISTRY.get_adapter(file_path.suffix)` then `adapter.parse(source, str(file_path))`. No changes needed here.
- `/Users/reza/work/loom/src/loom/indexer/pipeline.py` — `_resolve_module_file` delegates to `adapter.resolve_module_path`. No changes needed here.
- `/Users/reza/work/loom/src/loom/config.py` — `LoomConfig` computes `watch_extensions` and `excluded_dirs` from `REGISTRY` at instantiation time. Adding new adapters to the registry automatically expands these.
- `/Users/reza/work/loom/pyproject.toml` — `tree-sitter-python`, `tree-sitter-go`, `tree-sitter-java`, `tree-sitter-rust`, `tree-sitter-c-sharp` are already in `dependencies`. `mypy` overrides for all five already exist.

### Existing tests

- `/Users/reza/work/loom/tests/test_qa_adapter_arch.py` — Adversarial QA tests for the adapter-arch layer. The `TestRegressions.test_should_index_rejects_python_file` test currently asserts that `.py` files return `False` from `_should_index`, because Python is not yet registered. This test **will break** once the Python adapter is registered — it is the canary that signals registration is working. It must be updated as part of the registry task.
- `/Users/reza/work/loom/tests/test_parser.py` — JavaScript parser tests. No changes needed.

---

## Gaps & Needed Changes

### New files to create

**`src/loom/indexer/adapters/python.py`**
- Module-level: `import tree_sitter_python as tspy; PY_LANGUAGE = Language(tspy.language())`
- `PythonAdapter` class with `extensions = frozenset({".py", ".pyi"})`, `language_name = "python"`, `excluded_dirs = frozenset({"__pycache__", ".venv", "venv", ".tox", ".mypy_cache", ".pytest_cache"})`
- `parse()`: Walk AST for `function_definition` (kind="function"), `class_definition` (kind="class"), methods inside classes as `ClassName.method_name` (kind="method"), module-level UPPER_CASE assignments (kind="variable"), decorated functions/classes (capture decorator in context).
- `resolve_module_path()`: Try `import_path.py`, then `import_path/__init__.py`, then dot-to-slash conversion for dotted paths (`foo.bar` → `foo/bar.py`, `foo/bar/__init__.py`). Handle relative imports starting with `.` by resolving from `source_file`'s directory.

**`src/loom/indexer/adapters/go.py`**
- Module-level: `import tree_sitter_go as tsgo; GO_LANGUAGE = Language(tsgo.language())`
- `GoAdapter` class with `extensions = frozenset({".go"})`, `language_name = "go"`, `excluded_dirs = frozenset({"vendor"})`
- `parse()`: Walk for `function_declaration` (kind="function"), `method_declaration` with receiver type as `ReceiverType.method_name` (kind="method"), `type_declaration` containing `struct_type` or `interface_type` (kind="class"), `const_declaration` / `var_declaration` at package level (kind="variable"). Struct embedding → "extends" edge.
- `resolve_module_path()`: Go imports are package paths. Strip any module prefix and match the tail against directory paths in `known_files` (match files under `pkg/foo/` for `import "project/pkg/foo"`).

**`src/loom/indexer/adapters/java.py`**
- Module-level: `import tree_sitter_java as tsjava; JAVA_LANGUAGE = Language(tsjava.language())`
- `JavaAdapter` class with `extensions = frozenset({".java"})`, `language_name = "java"`, `excluded_dirs = frozenset({"target", "build", ".gradle", ".idea", "out"})`
- `parse()`: Walk for `class_declaration`, `interface_declaration`, `enum_declaration`, `record_declaration` (all kind="class"). Enum constants as kind="variable". Methods as `ClassName.methodName` (kind="method"). Static/public fields as `ClassName.fieldName` (kind="variable"). Annotations in context only. Inner classes as `OuterClass.InnerClass`.
- Edge extraction: `import` statements (skip wildcard `import pkg.*` with log warning), method calls, `new ClassName()` → "instantiates", `extends` → "extends", `implements` → "implements".
- `resolve_module_path()`: Convert `com.example.Foo` → `com/example/Foo.java` and search `known_files`. Fall back to tail-segment matching.

**`src/loom/indexer/adapters/rust.py`**
- Module-level: `import tree_sitter_rust as tsrust; RUST_LANGUAGE = Language(tsrust.language())`
- `RustAdapter` class with `extensions = frozenset({".rs"})`, `language_name = "rust"`, `excluded_dirs = frozenset({"target"})`
- `parse()`: Walk for `function_item` (kind="function"), `struct_item` (kind="class"), `enum_item` (kind="class") with variants as `EnumName.VariantName` (kind="variable"), `trait_item` (kind="class"), impl blocks extracting methods as `StructName.method` (kind="method"), `type_alias` (kind="variable"), `const_item`/`static_item` (kind="variable"), `macro_definition` (kind="macro").
- Edge extraction: `use` statements (skip glob `use ...*` with log warning), calls, `impl Trait for Struct` → "implements"/"implemented_by" edges.
- `resolve_module_path()`: `mod foo` → `foo.rs` then `foo/mod.rs`. `use crate::X` → project root. `use super::X` → parent directory.

**`src/loom/indexer/adapters/csharp.py`**
- Module-level: `import tree_sitter_c_sharp as tscs; CS_LANGUAGE = Language(tscs.language())`
- `CSharpAdapter` class with `extensions = frozenset({".cs"})`, `language_name = "csharp"`, `excluded_dirs = frozenset({"bin", "obj", ".vs", "packages"})`
- `parse()`: Walk for `class_declaration`, `struct_declaration`, `interface_declaration`, `enum_declaration`, `record_declaration` (all kind="class"). Enum members as `EnumName.MemberName` (kind="variable"). Methods as `ClassName.MethodName` (kind="method"). Properties as `ClassName.PropertyName` (kind="variable").
- Edge extraction: `using` directives (namespace, `using static`, `using Alias = Type`), method calls, `new ClassName()` → "instantiates", base class → "extends", interfaces → "implements". Partial classes merged by qualified name.
- `resolve_module_path()`: C# uses namespaces, not file paths. Fall through to strategies 4-5 (qualified and global name match) in the pipeline. Return `import_path` unchanged when no file match.

### Modified files

**`src/loom/indexer/adapters/__init__.py`**

Replace current single-adapter content with:
1. Import `AdapterRegistry` from base.
2. Create `REGISTRY`.
3. Each of the 6 adapters imported in a `try/except ImportError` block — log a warning and skip if grammar package missing. This prevents a missing grammar from killing the entire server.
4. Expose `get_adapter` and `get_all_extensions` as module-level convenience functions delegating to `REGISTRY` (as required by task 9).

**`tests/test_qa_adapter_arch.py`** — one targeted fix:
- `TestRegressions.test_should_index_rejects_python_file` currently asserts `_should_index(path_to_py_file, config) is False`. Once `PythonAdapter` is registered, this assertion flips to `True`. The test must be updated to assert `True`, or restructured to test a truly unregistered extension (e.g., `.rb`).

### New test file to create

**`tests/test_lang_adapters.py`**

Parametrized tests covering all 5 new adapters. Per-adapter sections must verify:
- Protocol conformance (`isinstance(adapter, LanguageAdapter)`)
- `parse()` extracts correct symbols (name, kind) from minimal valid source snippets
- `parse()` returns `([], [])` for wrong extension
- `parse()` does not raise on empty bytes or syntactically broken source
- `resolve_module_path()` returns unchanged path when nothing matches
- Registry integration: adapter is reachable from `REGISTRY.get_adapter(ext)` for each extension
- `LoomConfig` `watch_extensions` contains each new extension
- `LoomConfig` `excluded_dirs` contains each adapter's excluded dirs

---

## Integration Surface

### Protocol contract (must not change)

```python
class LanguageAdapter(Protocol):
    extensions: frozenset[str]
    language_name: str
    excluded_dirs: frozenset[str]
    def parse(self, source: bytes, file_path: str) -> tuple[list[Symbol], list[ParsedEdge]]: ...
    def resolve_module_path(self, import_path: str, source_file: str, known_files: set[str]) -> str: ...
```

### Registry API (must not change)

```python
REGISTRY.get_adapter(extension: str) -> LanguageAdapter | None
REGISTRY.get_all_extensions() -> frozenset[str]
REGISTRY.get_all_excluded_dirs() -> frozenset[str]
```

### `__init__.py` additions (task 9)

```python
def get_adapter(extension: str) -> LanguageAdapter | None:
    return REGISTRY.get_adapter(extension)

def get_all_extensions() -> frozenset[str]:
    return REGISTRY.get_all_extensions()
```

These are new additions — they don't break any existing consumers which import `REGISTRY` directly.

### Config expansion (automatic, no code change needed)

`LoomConfig.watch_extensions` and `excluded_dirs` are computed from `REGISTRY` at instantiation. All 5 new adapters' extensions and excluded dirs will flow through automatically once they are registered.

---

## Risks & Dependencies

1. **tree-sitter node-type names** — The node type strings for each grammar (e.g., `function_item`, `struct_item`) must be verified against the actual grammar. They are documented in task spec and consistent with tree-sitter grammar conventions, but should be confirmed by examining the grammar queries during implementation.

2. **impl block receiver type extraction (Rust)** — Extracting the struct name from `impl StructName { ... }` and `impl Trait for Struct { ... }` requires careful navigation of the Rust AST. The receiver type node is typically a `type_identifier` child of the `impl_item`.

3. **`TestRegressions.test_should_index_rejects_python_file` in `test_qa_adapter_arch.py`** — This is the one existing test that will break when Python adapter is registered. It's a known, intentional breakage that must be fixed in the same commit as the registry update.

4. **Grammar package import names** — The Python import for C# is `tree_sitter_c_sharp` (underscore, not hyphen), confirmed in `pyproject.toml` mypy overrides. All five are already confirmed: `tree_sitter_python`, `tree_sitter_go`, `tree_sitter_java`, `tree_sitter_rust`, `tree_sitter_c_sharp`.

5. **Coverage gate** — `pyproject.toml` requires 85% coverage. Five new adapter files add significant new lines; the test file must cover parse() and resolve_module_path() for each adapter to stay above the gate.

6. **Ordering constraint** — Registry update (`__init__.py`) should be done after all 5 adapters exist, so the try/except blocks have real modules to import. The developer can implement adapters in any order but should register incrementally (one adapter at a time) to keep tests passing throughout.

---

## Research Needed

None. All grammar packages are already in `pyproject.toml`. The `LanguageAdapter` protocol and `AdapterRegistry` are complete. The `JavaScriptAdapter` in `javascript.py` is the full reference implementation — no new libraries or APIs are needed.
