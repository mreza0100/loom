> Author: architect

# Architecture — lang-adapters

## Overview

Five new language adapters (Python, Go, Java, Rust, C#) conform to the `LanguageAdapter` Protocol already established by the adapter-arch pipeline. No changes to the Protocol, Registry, parser, pipeline, or config are needed — adding adapters to the registry is sufficient to expand all derived sets (`watch_extensions`, `excluded_dirs`). The work is exclusively:

1. Five new files in `src/loom/indexer/adapters/`
2. One updated file: `src/loom/indexer/adapters/__init__.py`
3. One updated test: `tests/test_qa_adapter_arch.py` (single assertion flip)
4. One new test file: `tests/test_lang_adapters.py`

---

## No-Research Decision

All grammar packages are confirmed installed and importable. All node type names below were verified by running the actual grammars against representative source snippets. No library research is needed — the JavaScript adapter is the sole reference pattern.

---

## Structural Pattern — All Adapters Must Follow JavaScriptAdapter Exactly

Every adapter follows this exact module structure:

```
Module-level Language init (eager, once at import time)
Module-level private constants (extensions frozenset, excluded_dirs frozenset)
Class with three frozenset class attributes
  parse() — validates extension, creates Parser, calls _walk_node
  resolve_module_path() — ordered candidate probing
Module-private _walk_node and helpers (no class methods)
```

The `Parser(LANGUAGE)` is instantiated **inside** `parse()`, not at module level. The module-level `LANGUAGE` object is created once at import time. This matches `javascript.py` precisely.

---

## File Structure

```
src/loom/indexer/adapters/
├── __init__.py          (modified)
├── base.py              (unchanged)
├── javascript.py        (unchanged)
├── python.py            (new)
├── go.py                (new)
├── java.py              (new)
├── rust.py              (new)
└── csharp.py            (new)

tests/
├── test_qa_adapter_arch.py    (one assertion updated)
└── test_lang_adapters.py      (new)
```

---

## Module 1 — `src/loom/indexer/adapters/python.py`

### Module-level

```python
import tree_sitter_python as tspy
PY_LANGUAGE = Language(tspy.language())

_PY_EXTENSIONS: frozenset[str] = frozenset({".py", ".pyi"})
_PY_EXCLUDED_DIRS: frozenset[str] = frozenset({
    "__pycache__", ".venv", "venv", ".tox", ".mypy_cache", ".pytest_cache"
})
```

### `PythonAdapter` class attributes

```python
extensions: frozenset[str] = _PY_EXTENSIONS
language_name: str = "python"
excluded_dirs: frozenset[str] = _PY_EXCLUDED_DIRS
```

### `parse()` logic

Extension guard: return `([], [])` if `Path(file_path).suffix` not in `_PY_EXTENSIONS`.

`_walk_node(node, source, file_path, symbols, edges, class_stack)` — the `class_stack` is a `list[str]` tracking enclosing class names for method and inner-class qualification. Walk children recursively; these node types are handled:

| Node type | Action |
|-----------|--------|
| `import_statement` | Extract `dotted_name` child → `"imports"` edge |
| `import_from_statement` | Extract module from `dotted_name` / `relative_import` child, extract imported names from subsequent `dotted_name` children → `"imports"` edges |
| `decorated_definition` | Extract decorator name from `decorator` child's `identifier`; unwrap inner `function_definition` or `class_definition`; pass decorator text as part of `context` |
| `function_definition` | If `class_stack` non-empty: symbol name = `ClassName.func_name`, kind=`"method"`. Else: kind=`"function"`. Extract calls from body via `_extract_calls()`. **Do not recurse into body for further symbol discovery** (matches JS pattern — inner functions are not indexed). |
| `class_definition` | Symbol kind=`"class"`. Push class name onto `class_stack`. Extract parents from `argument_list` child's `identifier` children → `"extends"` + `"extended_by"` edges. Recurse into `block` with updated `class_stack`. Pop after recursion. |
| `expression_statement` → `assignment` | At module level (class_stack empty): if LHS `identifier` text is `UPPER_CASE` (all uppercase, at least one char) → kind=`"variable"` |

**Verified node types (from grammar inspection):**
- Function: `function_definition`, name child: `identifier` at index 1
- Class: `class_definition`, name child: `identifier` at index 1, parents in `argument_list` children
- Module import: `import_statement`, module in `dotted_name` child
- From import: `import_from_statement`, module from `dotted_name` or `relative_import`, names from trailing `dotted_name` children
- Decorated: `decorated_definition` wrapping `function_definition` or `class_definition`, decorator in `decorator` → `identifier`
- Assignment: `expression_statement` → `assignment` → LHS `identifier`
- Calls inside bodies: `call` node, function via `identifier` or `attribute` child

**Call extraction for Python (`_extract_calls`):**
- `call` node: function is `identifier` child → plain call edge. `attribute` child → method call: if object is `self` or `cls`, emit `ClassName.method_name`; otherwise emit `obj.method_name` raw. If the callee (bare identifier) is a known UPPER_CASE-named class (i.e., `Foo()` where Foo is capitalized), emit `"instantiates"` edge instead of `"calls"`.
- Recurse into all child nodes.

### `resolve_module_path()` logic

Ordered resolution:

1. Direct match: `import_path` in `known_files` → return it
2. Convert dots to slashes: `foo.bar` → try `foo/bar.py` then `foo/bar/__init__.py`
3. Relative import (starts with `.`): strip leading dots, count them as parent levels from `source_file`'s directory; reconstruct absolute path candidates with `.py` / `/__init__.py`
4. Return `import_path` unchanged if no match

---

## Module 2 — `src/loom/indexer/adapters/go.py`

### Module-level

```python
import tree_sitter_go as tsgo
GO_LANGUAGE = Language(tsgo.language())

_GO_EXTENSIONS: frozenset[str] = frozenset({".go"})
_GO_EXCLUDED_DIRS: frozenset[str] = frozenset({"vendor"})
```

### `GoAdapter` class attributes

```python
extensions: frozenset[str] = _GO_EXTENSIONS
language_name: str = "go"
excluded_dirs: frozenset[str] = _GO_EXCLUDED_DIRS
```

### `parse()` logic

Extension guard: return `([], [])` if suffix not `.go`.

`_walk_node(node, source, file_path, symbols, edges)` — flat walk (no class_stack; Go has no nested types):

| Node type | Action |
|-----------|--------|
| `import_declaration` | Walk children: single `import_spec` → extract `interpreted_string_literal_content`; grouped `import_spec_list` → extract each `import_spec` → `"imports"` edge |
| `function_declaration` | Name from `identifier` child → kind=`"function"`. Extract calls from `block`. |
| `method_declaration` | Receiver type from first `parameter_list` → `parameter_declaration` → type is `type_identifier` or `pointer_type` → `type_identifier`. Method name from `field_identifier` child. Symbol name = `ReceiverType.method_name`, kind=`"method"`. Extract calls. |
| `type_declaration` | Walk `type_spec` child. If it has `struct_type` child → kind=`"class"`. If it has `interface_type` child → kind=`"class"`. If it has `type_alias` child → kind=`"variable"`. Name from `type_identifier` child of `type_spec`. For structs: scan `field_declaration_list` for embedded fields (a `field_declaration` whose only named child is a `type_identifier`, no `field_identifier`) → `"extends"` + `"extended_by"` edge to the embedded type name. |
| `const_declaration` | Walk `const_spec` children → `identifier` child → kind=`"variable"` |
| `var_declaration` | Walk `var_spec` children → `identifier` child → kind=`"variable"` |

**Verified node types:**
- Function: `function_declaration`, name: `identifier`
- Method: `method_declaration`, receiver in first `parameter_list` → `parameter_declaration` → `type_identifier` (or `pointer_type` → `type_identifier`), method name: `field_identifier`
- Struct: `type_declaration` → `type_spec` containing `struct_type`
- Interface: `type_declaration` → `type_spec` containing `interface_type`
- Type alias: `type_declaration` → `type_alias` (distinct node type, not `type_spec`)
- Grouped import: `import_declaration` → `import_spec_list` → `import_spec` → `interpreted_string_literal` → `interpreted_string_literal_content`
- Single import: `import_declaration` → `import_spec` → `interpreted_string_literal` → `interpreted_string_literal_content`
- Embedding: `field_declaration` inside `field_declaration_list` with only a `type_identifier` child (no `field_identifier`)
- Calls: `call_expression`, function is `identifier` or `selector_expression`

**Call extraction for Go (`_extract_calls`):**
- `call_expression` node: function child is `identifier` → plain call. `selector_expression` → emit `pkg.Method` as target name.
- Recurse into all children.

### `resolve_module_path()` logic

Go import paths are package paths (e.g., `"github.com/example/pkg/util"`). Resolution:

1. Direct match in `known_files` → return it
2. Try matching the final path segment(s) as a directory prefix: for progressively longer tail suffixes of the import path split by `/`, check if any file in `known_files` starts with that suffix + `/`; return the first match
3. Return `import_path` unchanged

---

## Module 3 — `src/loom/indexer/adapters/java.py`

### Module-level

```python
import tree_sitter_java as tsjava
JAVA_LANGUAGE = Language(tsjava.language())

_JAVA_EXTENSIONS: frozenset[str] = frozenset({".java"})
_JAVA_EXCLUDED_DIRS: frozenset[str] = frozenset({
    "target", "build", ".gradle", ".idea", "out"
})
```

### `JavaAdapter` class attributes

```python
extensions: frozenset[str] = _JAVA_EXTENSIONS
language_name: str = "java"
excluded_dirs: frozenset[str] = _JAVA_EXCLUDED_DIRS
```

### `parse()` logic

Extension guard: return `([], [])` if suffix not `.java`.

`_walk_node(node, source, file_path, symbols, edges, class_stack)` — `class_stack: list[str]` for qualified names:

| Node type | Action |
|-----------|--------|
| `import_declaration` | If an `asterisk` child is present → log warning and skip. Else: read `scoped_identifier` text → `"imports"` edge. For `static` child present: include full scoped identifier text. |
| `class_declaration` | Name from `identifier` child. Qualified name = `OuterClass.ClassName` if `class_stack` non-empty. Kind=`"class"`. Extract `superclass` child → `type_identifier` → `"extends"` + `"extended_by"` edges. Extract `super_interfaces` → `type_list` → `type_identifier` children → `"implements"` edges. Push to `class_stack`, recurse into `class_body`, pop. |
| `interface_declaration` | Same as `class_declaration` but check `extends_interfaces` → `type_list` → `"extends"` edges. Kind=`"class"`. |
| `enum_declaration` | Name → kind=`"class"`. Recurse into `enum_body`: `enum_constant` children → `identifier` → emit as `EnumName.CONSTANT_NAME` kind=`"variable"`. |
| `record_declaration` | Name → kind=`"class"`. |
| `method_declaration` | Name from `identifier` child. Qualified: `ClassName.methodName`. Kind=`"method"`. Extract calls from `block`. |
| `constructor_declaration` | Name from `identifier`. Qualified: `ClassName.ConstructorName`. Kind=`"method"`. Extract calls from `constructor_body`. |
| `field_declaration` | Name from `variable_declarator` → `identifier`. Qualified: `ClassName.fieldName`. Kind=`"variable"`. Only extract if in a class context (`class_stack` non-empty). |

**Verified node types:**
- Class: `class_declaration`, name: `identifier`, base: `superclass` → `type_identifier`, interfaces: `super_interfaces` → `type_list` → `type_identifier`
- Interface: `interface_declaration`, extends: `extends_interfaces` → `type_list`
- Enum: `enum_declaration`, body: `enum_body`, constants: `enum_constant` → `identifier`
- Record: `record_declaration`
- Methods: `method_declaration`, name: `identifier`
- Constructor: `constructor_declaration`
- Fields: `field_declaration` → `variable_declarator` → `identifier`
- Wildcard import: `import_declaration` with `asterisk` child
- Calls: `method_invocation` node, object: `identifier` or `scoped_identifier`, method: `identifier`
- Instantiation: `object_creation_expression`, class: `type_identifier`

**Call extraction for Java (`_extract_calls`):**
- `method_invocation`: extract method `identifier` as target name. If preceded by an `identifier` or scoped object, emit `Object.method` form.
- `object_creation_expression`: `type_identifier` child → `"instantiates"` edge.
- Recurse into all children.

### `resolve_module_path()` logic

1. Convert `com.example.Foo` → `com/example/Foo.java` and check `known_files`
2. Try tail-segment matching: check if any file in `known_files` ends with `/{LastSegment}.java`
3. Return `import_path` unchanged

---

## Module 4 — `src/loom/indexer/adapters/rust.py`

### Module-level

```python
import tree_sitter_rust as tsrust
RUST_LANGUAGE = Language(tsrust.language())

_RUST_EXTENSIONS: frozenset[str] = frozenset({".rs"})
_RUST_EXCLUDED_DIRS: frozenset[str] = frozenset({"target"})
```

### `RustAdapter` class attributes

```python
extensions: frozenset[str] = _RUST_EXTENSIONS
language_name: str = "rust"
excluded_dirs: frozenset[str] = _RUST_EXCLUDED_DIRS
```

### `parse()` logic

Extension guard: return `([], [])` if suffix not `.rs`.

`_walk_node(node, source, file_path, symbols, edges, impl_type)` — `impl_type: str | None` carries the struct/trait context inside `impl_item` blocks. Top-level walk is flat; `impl_item` sets `impl_type` for its children.

| Node type | Action |
|-----------|--------|
| `use_declaration` | If child is `use_wildcard` → log warning and skip. If child is `scoped_identifier` → extract last `identifier` as symbol name, full path as `target_file` hint → `"imports"` edge. If child is `scoped_use_list` → extract `use_list` → each `identifier` → separate `"imports"` edges. |
| `function_item` | If `impl_type` non-None: name = `impl_type.method_name`, kind=`"method"`. Else kind=`"function"`. Extract calls from `block`. |
| `struct_item` | Name from `type_identifier` child → kind=`"class"`. |
| `enum_item` | Name from `type_identifier` child → kind=`"class"`. Walk `enum_variant_list` → each `enum_variant` → `identifier` → emit `EnumName.VariantName` kind=`"variable"`. |
| `trait_item` | Name from `type_identifier` child → kind=`"class"`. Walk `declaration_list` → `function_signature_item` children as method stubs (kind=`"method"`, name=`TraitName.method`). |
| `impl_item` | Two forms: `impl StructName { ... }` (children: `type_identifier`, `declaration_list`) and `impl TraitName for StructName { ... }` (children: `type_identifier`, `for`, `type_identifier`, `declaration_list`). For the second form: emit `StructName` `"implements"` `TraitName` edge and `TraitName` `"implemented_by"` `StructName` edge. In both cases: set `impl_type` to the struct name (last `type_identifier` before `declaration_list`) and recurse into `declaration_list`. |
| `type_item` | Name from `type_identifier` child → kind=`"variable"` |
| `const_item` | Name from `identifier` child → kind=`"variable"` |
| `static_item` | Name from `identifier` child → kind=`"variable"` |
| `macro_definition` | Name from `identifier` child → kind=`"macro"` |

**Verified node types:**
- Function: `function_item`, name: `identifier`
- Struct: `struct_item`, name: `type_identifier`
- Enum: `enum_item`, name: `type_identifier`, variants: `enum_variant_list` → `enum_variant` → `identifier`
- Trait: `trait_item`, name: `type_identifier`, body: `declaration_list`
- Trait method stubs: `function_signature_item` inside trait `declaration_list`
- Impl block: `impl_item`, plain: `[type_identifier, declaration_list]`; with trait: `[type_identifier, "for", type_identifier, declaration_list]`
- Type alias: `type_item`, name: `type_identifier`
- Const: `const_item`, name: `identifier`
- Static: `static_item`, name: `identifier`
- Macro: `macro_definition`, name: `identifier`
- Use: `use_declaration`, glob: `use_wildcard` child, grouped: `scoped_use_list` → `use_list`
- Calls: `call_expression`, function: `identifier` or `scoped_identifier` or `field_expression`

**Call extraction for Rust (`_extract_calls`):**
- `call_expression`: function child `identifier` → plain call. `scoped_identifier` → emit full path. `field_expression` → method chain.
- Recurse into all children.

**Impl block type extraction detail:**

For `impl_item`, iterate children and collect `type_identifier` nodes. If `for` keyword child present (node type `"for"`): first `type_identifier` = trait name, second `type_identifier` = struct name. Otherwise single `type_identifier` = struct name.

### `resolve_module_path()` logic

1. Direct match in `known_files` → return it
2. Strip `crate::` prefix → treat remainder as path from project root with `/` separators; try `path.rs` then `path/mod.rs`
3. Strip `super::` prefix → resolve from `source_file`'s parent directory; try `name.rs` then `name/mod.rs`
4. `mod foo` style (no `::` prefix): try `{dir}/foo.rs` then `{dir}/foo/mod.rs` where `dir` is `source_file`'s directory
5. Return `import_path` unchanged

---

## Module 5 — `src/loom/indexer/adapters/csharp.py`

### Module-level

```python
import tree_sitter_c_sharp as tscs
CS_LANGUAGE = Language(tscs.language())

_CS_EXTENSIONS: frozenset[str] = frozenset({".cs"})
_CS_EXCLUDED_DIRS: frozenset[str] = frozenset({"bin", "obj", ".vs", "packages"})
```

### `CSharpAdapter` class attributes

```python
extensions: frozenset[str] = _CS_EXTENSIONS
language_name: str = "csharp"
excluded_dirs: frozenset[str] = _CS_EXCLUDED_DIRS
```

### `parse()` logic

Extension guard: return `([], [])` if suffix not `.cs`.

`_walk_node(node, source, file_path, symbols, edges, class_stack)` — `class_stack: list[str]`:

| Node type | Action |
|-----------|--------|
| `using_directive` | Extract namespace/type from `identifier` or `qualified_name` child text → `"imports"` edge. For `using Alias = Type` form (has `=` child), use the RHS `qualified_name` as target. |
| `namespace_declaration` | Recurse into `declaration_list` body (don't emit a symbol for the namespace itself). |
| `class_declaration` | Name from `identifier` child. Qualified if `class_stack` non-empty. Kind=`"class"`. Extract `base_list` → all `identifier` / `qualified_name` children (skipping `:` and `,` tokens): first entry is `"extends"` candidate (emit `"extends"` + `"extended_by"`), remaining are `"implements"` edges. **Note: C# AST does not distinguish base class from interfaces in `base_list` — all entries are treated as `"extends"` for simplicity unless the entry name starts with `I` by convention. This is a known trade-off.** Push to `class_stack`, recurse into `declaration_list`, pop. |
| `struct_declaration` | Same as `class_declaration`, kind=`"class"`. |
| `interface_declaration` | Name → kind=`"class"`. `base_list` entries → `"extends"` edges. |
| `enum_declaration` | Name → kind=`"class"`. Walk `enum_member_declaration_list` → `enum_member_declaration` → `identifier` → emit `EnumName.MemberName` kind=`"variable"`. |
| `record_declaration` | Name → kind=`"class"`. |
| `method_declaration` | Name from `identifier` child. Qualified: `ClassName.MethodName`. Kind=`"method"`. Extract calls from `block`. |
| `constructor_declaration` | Name from `identifier`. Qualified: `ClassName.ConstructorName`. Kind=`"method"`. |
| `property_declaration` | Name from `identifier`. Qualified: `ClassName.PropertyName`. Kind=`"variable"`. |
| `field_declaration` | Name from `variable_declaration` → `variable_declarator` → `identifier`. Qualified. Kind=`"variable"`. Only when `class_stack` non-empty. |

**Verified node types:**
- Class: `class_declaration`, name: `identifier`, base: `base_list` → `identifier` children
- Struct: `struct_declaration`
- Interface: `interface_declaration`, base: `base_list`
- Enum: `enum_declaration`, body: `enum_member_declaration_list` → `enum_member_declaration` → `identifier`
- Record: `record_declaration`
- Method: `method_declaration`, name: `identifier`
- Constructor: `constructor_declaration`
- Property: `property_declaration`, name: `identifier`
- Field: `field_declaration` → `variable_declaration` → `variable_declarator` → `identifier`
- Using: `using_directive`, content: `identifier` or `qualified_name`
- Namespace: `namespace_declaration`, body: `declaration_list`
- Calls: `invocation_expression`, function: `member_access_expression` → `identifier` (method name), or bare `identifier`
- Instantiation: `object_creation_expression`, class: `identifier` or `qualified_name`

**C# base_list trade-off:** The `base_list` AST node contains all base types (class + interfaces) as a flat list of identifiers separated by commas. The grammar does not encode whether each entry is a class or interface — that information is semantic, not syntactic. The architecture decision is: emit `"extends"` for all entries from a `class_declaration`'s `base_list`. This produces false "extends" edges for implemented interfaces, but is honest about what the AST provides and keeps the adapter simple. A future enhancement could apply `I`-prefix heuristics.

**Call extraction for C# (`_extract_calls`):**
- `invocation_expression`: function child `member_access_expression` → last `identifier` as method name; emit `Object.Method` form. Bare `identifier` → plain call.
- `object_creation_expression`: `identifier` or `qualified_name` child → `"instantiates"` edge.
- Recurse into all children.

### `resolve_module_path()` logic

C# uses namespaces, not file paths. File-to-symbol resolution is namespace-based and cannot be reliably performed from import paths alone:

1. Direct match in `known_files` → return it (unlikely but safe)
2. Return `import_path` unchanged

The pipeline's strategy 4-5 (global symbol name match) handles C# cross-file resolution downstream.

---

## Module 6 — `src/loom/indexer/adapters/__init__.py` (modified)

Replace the current content with:

```python
"""Adapter registry singleton — imports and registers all language adapters."""

import logging
from loom.indexer.adapters.base import AdapterRegistry, LanguageAdapter

log = logging.getLogger(__name__)

REGISTRY: AdapterRegistry = AdapterRegistry()

# Each adapter imported in try/except — missing grammar skips the adapter without
# killing the server. ImportError only; do not catch broad exceptions.

try:
    from loom.indexer.adapters.javascript import JavaScriptAdapter
    REGISTRY.register(JavaScriptAdapter())
except ImportError:
    log.warning("tree-sitter-javascript not available; JS/TS files will not be indexed")

try:
    from loom.indexer.adapters.python import PythonAdapter
    REGISTRY.register(PythonAdapter())
except ImportError:
    log.warning("tree-sitter-python not available; Python files will not be indexed")

try:
    from loom.indexer.adapters.go import GoAdapter
    REGISTRY.register(GoAdapter())
except ImportError:
    log.warning("tree-sitter-go not available; Go files will not be indexed")

try:
    from loom.indexer.adapters.java import JavaAdapter
    REGISTRY.register(JavaAdapter())
except ImportError:
    log.warning("tree-sitter-java not available; Java files will not be indexed")

try:
    from loom.indexer.adapters.rust import RustAdapter
    REGISTRY.register(RustAdapter())
except ImportError:
    log.warning("tree-sitter-rust not available; Rust files will not be indexed")

try:
    from loom.indexer.adapters.csharp import CSharpAdapter
    REGISTRY.register(CSharpAdapter())
except ImportError:
    log.warning("tree-sitter-c-sharp not available; C# files will not be indexed")


def get_adapter(extension: str) -> LanguageAdapter | None:
    return REGISTRY.get_adapter(extension)


def get_all_extensions() -> frozenset[str]:
    return REGISTRY.get_all_extensions()
```

---

## Test Changes

### `tests/test_qa_adapter_arch.py` — single fix

`TestRegressions.test_should_index_rejects_python_file`: flip the assertion from `is False` to `is True`. The test name should also be updated to `test_should_index_accepts_python_file` to remain semantically accurate.

Additionally, `TestPipelineResolveModuleFileFallback.test_unknown_extension_returns_target_unchanged` currently uses `.py` as the "unregistered" extension in its comment. The test logic itself passes `"src/main.py"` as source file and expects `None` adapter → result unchanged. Once Python is registered, this test will break because `_resolve_module_file` will find an adapter for `.py`. The test must be updated to use a genuinely unregistered extension (`.rb`) instead of `.py`.

### `tests/test_lang_adapters.py` — new file

Structure: parametrized where possible, per-adapter sections where not. Required coverage per adapter:

1. **Protocol conformance** — `isinstance(adapter, LanguageAdapter)`
2. **Extension guard** — `parse()` returns `([], [])` for wrong extension
3. **Empty source** — `parse(b"", "file.ext")` returns `([], [])` without raising
4. **Broken source** — `parse(b"{{{{", "file.ext")` returns lists without raising
5. **Symbol extraction** — minimal valid snippet; assert expected `(name, kind)` pairs present
6. **Edge extraction** — minimal valid snippet; assert expected `ParsedEdge` relationships present
7. **resolve_module_path no-match** — returns `import_path` unchanged when `known_files` is empty
8. **Registry integration** — `REGISTRY.get_adapter(ext)` returns the adapter for each extension
9. **Config propagation** — `LoomConfig(target_dir=tmp_path).watch_extensions` contains each extension
10. **Excluded dirs** — `LoomConfig(target_dir=tmp_path).excluded_dirs` contains adapter's excluded dirs

Minimal source snippets to use per adapter (sufficient to hit primary symbol types):

**Python:**
```python
b"def foo(): pass\nclass Bar: pass\nMAX = 1\nimport os\nfrom pathlib import Path"
```

**Go:**
```go
b"package main\nimport \"fmt\"\nconst X = 1\nfunc F() {}\ntype S struct{}\nfunc (s S) M() {}"
```

**Java:**
```java
b"import com.example.Foo;\nclass C extends Base implements I {\n  public void m() {}\n}"
```

**Rust:**
```rust
b"use crate::x;\nfn f() {}\nstruct S {}\ntrait T {}\nimpl S { fn m(&self) {} }\nimpl T for S { fn m(&self) {} }"
```

**C#:**
```csharp
b"using System;\nclass C : Base {\n  public void M() {}\n  public string P { get; }\n}"
```

---

## Data Flow (unchanged from adapter-arch)

```
parse_file(path) → REGISTRY.get_adapter(suffix) → adapter.parse(source, path)
                                                    → (symbols, edges)

pipeline._resolve_module_file(target, known, source_file)
    → REGISTRY.get_adapter(source_file suffix)
    → adapter.resolve_module_path(target, source_file, known_files)
```

No changes to `parser.py` or `pipeline.py` required.

---

## Trade-off Decisions

### 1. UPPER_CASE heuristic for Python module variables

Only module-level assignments where the LHS identifier is all-uppercase are indexed as `kind="variable"`. This excludes private constants (`_CONST`), dunder attributes (`__version__`), and regular variables. It targets the most indexing-valuable Python constants while avoiding noise.

**Rationale:** Indexing every module-level assignment in a large Python codebase would create thousands of low-signal variables. UPPER_CASE is the Python convention for public module constants.

### 2. C# base_list — no class/interface distinction

C# `base_list` contains both the base class and implemented interfaces in a flat list. The AST does not distinguish them. All entries are emitted as `"extends"` edges from a `class_declaration`. This means implemented interfaces will appear as "extends" relationships rather than "implements".

**Rationale:** Emitting wrong relationship type is better than omitting the relationship entirely. The coupling score computation can treat `"extends"` and `"implements"` similarly. A future adapter iteration can apply I-prefix heuristics or type-inference if precision matters.

### 3. No inner-function indexing

Following the JavaScript adapter pattern, functions nested inside other functions are not indexed as symbols. Only top-level functions, class methods, and module-level constants are indexed.

**Rationale:** Inner functions are implementation details. Indexing them inflates the symbol count with low-relatedness entries and adds noise to search results.

### 4. Rust impl block — method naming uses struct type, not trait type

When processing `impl Trait for Struct`, methods are emitted as `Struct.method_name` (not `Trait.method_name`). This matches the call-site naming (`s.method()` where `s: Struct`) and makes coupling resolution accurate.

### 5. Go `_test.go` files indexed

Go test files are included (the extension is `.go`; no suffix filtering). This is intentional — test files contain meaningful symbols that reveal coupling.

### 6. Java constructor indexed as kind="method"

Java constructors are indexed as `ClassName.ConstructorName` with kind=`"method"`. This is a slight approximation (constructors are not methods) but keeps the kind set small and predictable.

---

## Implementation Order (recommended)

The five adapters are independent of each other. The registry update (`__init__.py`) should come last. Suggested order:

1. `python.py` (most familiar to the Python developer, good warm-up)
2. `go.py` (flat AST, simplest structure)
3. `java.py` (most node types, but all verified)
4. `rust.py` (impl block handling is the tricky part)
5. `csharp.py` (namespace traversal, partial class note)
6. `__init__.py` (register all five + JS)
7. `test_lang_adapters.py` (new tests)
8. `test_qa_adapter_arch.py` (two assertion fixes)

The developer may implement in any order, but registering incrementally (add one adapter → run tests → add next) keeps the test suite green throughout.

---

## Coverage Gate Note

`pyproject.toml` requires 85% coverage. The five new adapter files will add ~800 lines of new code. `test_lang_adapters.py` must cover the happy path for each adapter's `parse()` and `resolve_module_path()` plus the extension guard and empty-source cases. The adversarial cases (binary garbage, very large source) from `test_qa_adapter_arch.py` style are recommended but not required for the gate.
