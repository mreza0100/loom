# Pipeline: lang-adapters

Implement 5 language adapters (Python, Go, Java, Rust, C#) using the LanguageAdapter protocol established by the adapter-arch pipeline. Each adapter lives in `src/loom/indexer/adapters/` and uses its respective tree-sitter grammar. Also update the adapter registry `__init__.py` to register all adapters.

## Task 4: Python adapter

File: `src/loom/indexer/adapters/python.py`
Grammar: `tree-sitter-python`
Extensions: `.py`, `.pyi`
Language name: `"python"`
Excluded dirs: `__pycache__`, `.venv`, `venv`, `.tox`, `.mypy_cache`, `.pytest_cache`

Symbol extraction:
- Functions (`function_definition`) — name, kind="function", with line/end_line/context
- Classes (`class_definition`) — name, kind="class"
- Methods inside classes as `ClassName.method_name`, kind="method"
- Module-level variables: UPPER_CASE assignments as kind="variable"
- Decorated functions/classes: capture decorator name in context

Edge extraction:
- Imports: `import X` → edge to X, `from X import Y` → edge to Y in module X, `from . import X` (relative imports) → resolve relative path
- Calls: function calls, method calls (`self.method()` → target `ClassName.method`), `cls.method()`
- Instantiation: `ClassName()` → "instantiates" edge
- Inheritance: `class Child(Parent)` → "extends"/"extended_by" edges, multiple parents supported

Module resolution:
- Try `import_path.py`, then `import_path/__init__.py`
- Relative imports: resolve from source file's directory
- Dot-separated paths: `from foo.bar import baz` → try `foo/bar.py` then `foo/bar/__init__.py`

## Task 5: Go adapter

File: `src/loom/indexer/adapters/go.py`
Grammar: `tree-sitter-go`
Extensions: `.go`
Language name: `"go"`
Excluded dirs: `vendor`

Symbol extraction:
- Functions (`function_declaration`) — kind="function"
- Methods with receiver (`method_declaration`) — stored as `ReceiverType.method_name`, kind="method"
- Structs (`type_declaration` with `struct_type`) — kind="class" (closest analog)
- Interfaces (`type_declaration` with `interface_type`) — kind="class"
- Type aliases — kind="variable"
- Package-level constants (`const_declaration`) and variables (`var_declaration`) — kind="variable"

Edge extraction:
- Imports: `import "path"` and grouped `import (...)` — single clean format
- Calls: function calls, method calls
- Struct embedding (`type A struct { B }`) → "extends" edge (closest analog)

Module resolution:
- Go imports are package paths — resolve to directory containing `.go` files
- Match `import "project/pkg/foo"` to directory `pkg/foo/` and symbols from any `.go` file there

Notes:
- `_test.go` files should be indexed
- No interface satisfaction detection (structural typing — can't extract from AST)
- No generics type-parameter extraction

## Task 6: Java adapter

File: `src/loom/indexer/adapters/java.py`
Grammar: `tree-sitter-java`
Extensions: `.java`
Language name: `"java"`
Excluded dirs: `target`, `build`, `.gradle`, `.idea`, `out`

Symbol extraction:
- Classes (`class_declaration`) — kind="class"
- Interfaces (`interface_declaration`) — kind="class"
- Enums (`enum_declaration`) — kind="class", enum constants as kind="variable"
- Records (`record_declaration`) — kind="class"
- Methods as `ClassName.methodName` — kind="method"
- Static/public fields as `ClassName.fieldName` — kind="variable"
- Annotations captured in context, not as symbols

Edge extraction:
- Imports: `import pkg.Class`, `import static pkg.Class.method` — skip `import pkg.*` (log warning)
- Calls: method calls, static method calls
- Instantiation: `new ClassName()`
- Inheritance: `extends` → "extends", `implements` → "implements" (separate relationship types)

Module resolution:
- `import com.example.Foo` → try matching tail segments against known files
- `com/example/Foo.java` path construction from package name

Notes:
- Inner classes as `OuterClass.InnerClass`
- Method overloads share name — disambiguated by line numbers

## Task 7: Rust adapter

File: `src/loom/indexer/adapters/rust.py`
Grammar: `tree-sitter-rust`
Extensions: `.rs`
Language name: `"rust"`
Excluded dirs: `target`

Symbol extraction:
- Functions (`function_item`) — kind="function"
- Structs (`struct_item`) — kind="class"
- Enums (`enum_item`) — kind="class", variants as `EnumName.VariantName` kind="variable"
- Traits (`trait_item`) — kind="class"
- Impl blocks: methods extracted as `StructName.method` — kind="method"
- Type aliases — kind="variable"
- Constants (`const_item`) and statics (`static_item`) — kind="variable"
- Macros (`macro_definition`) — kind="macro"

Edge extraction:
- `use` statements: `use crate::path::Symbol`, `pub use` re-exports — skip `use ...*` (log warning)
- Calls: function calls, method calls
- Trait implementations: `impl Trait for Struct` → "implements"/"implemented_by" edges

Module resolution:
- `mod foo` → try `foo.rs` then `foo/mod.rs`
- `use crate::X` → project root
- `use super::X` → parent module directory

Notes:
- Multiple `impl` blocks for same struct across files all contribute methods
- No lifetime/generic parameter extraction
- No procedural macro expansion

## Task 8: C# adapter

File: `src/loom/indexer/adapters/csharp.py`
Grammar: `tree-sitter-c-sharp`
Extensions: `.cs`
Language name: `"csharp"`
Excluded dirs: `bin`, `obj`, `.vs`, `packages`

Symbol extraction:
- Classes (`class_declaration`) — kind="class"
- Structs (`struct_declaration`) — kind="class"
- Interfaces (`interface_declaration`) — kind="class"
- Enums (`enum_declaration`) — kind="class", members as `EnumName.MemberName` kind="variable"
- Records (`record_declaration`) — kind="class"
- Methods as `ClassName.MethodName` — kind="method"
- Properties as `ClassName.PropertyName` — kind="variable"

Edge extraction:
- `using Namespace` directives, `using static Class`, `using Alias = Type`
- Calls: method calls, static method calls
- Instantiation: `new ClassName()`
- Inheritance: base class → "extends", interfaces → "implements"

Module resolution:
- C# uses namespaces not file paths — match class names against globally known symbols
- Resolution falls through to strategies 4-5 (qualified and global name match)

Notes:
- `partial class` across files: merge methods by qualified name
- Properties are single symbols (not split into get/set)
- No LINQ expression analysis, no extension method resolution

## Task 9 (partial): Adapter registry __init__.py

Update `src/loom/indexer/adapters/__init__.py` to:
1. Import and register all 6 adapters (JavaScript + 5 new)
2. Each import wrapped in try/except ImportError — log warning if grammar missing, skip adapter
3. Expose `get_adapter(extension)` and `get_all_extensions()` convenience functions

Wave: multi-lang-adapters
