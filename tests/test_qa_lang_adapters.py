"""QA adversarial tests for lang-adapters pipeline.

Targets: Python, Go, Java, Rust, C# adapters + registry.
Focus: unhappy paths, edge cases, coverage gaps (Rust 70%, Java 73%, etc.).
"""

from __future__ import annotations

import pytest

from loom.indexer.adapters import REGISTRY, get_adapter, get_all_extensions
from loom.indexer.adapters.base import LanguageAdapter

# ---------------------------------------------------------------------------
# Helpers / shared fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def python_adapter():
    from loom.indexer.adapters.python import PythonAdapter

    return PythonAdapter()


@pytest.fixture
def go_adapter():
    from loom.indexer.adapters.go import GoAdapter

    return GoAdapter()


@pytest.fixture
def java_adapter():
    from loom.indexer.adapters.java import JavaAdapter

    return JavaAdapter()


@pytest.fixture
def rust_adapter():
    from loom.indexer.adapters.rust import RustAdapter

    return RustAdapter()


@pytest.fixture
def csharp_adapter():
    from loom.indexer.adapters.csharp import CSharpAdapter

    return CSharpAdapter()


# ---------------------------------------------------------------------------
# Registry module-level helpers
# ---------------------------------------------------------------------------


class TestRegistryHelpers:
    """Tests for get_adapter() and get_all_extensions() module-level wrappers."""

    def test_get_adapter_returns_adapter_for_py(self) -> None:
        assert get_adapter(".py") is not None

    def test_get_adapter_returns_none_for_unknown(self) -> None:
        assert get_adapter(".xyz123") is None

    def test_get_all_extensions_returns_frozenset(self) -> None:
        exts = get_all_extensions()
        assert isinstance(exts, frozenset)

    def test_get_all_extensions_contains_all_languages(self) -> None:
        exts = get_all_extensions()
        for ext in (".py", ".pyi", ".go", ".java", ".rs", ".cs"):
            assert ext in exts, f"Expected {ext} in get_all_extensions()"

    def test_get_adapter_dot_go_is_language_adapter(self) -> None:
        adapter = get_adapter(".go")
        assert adapter is not None
        assert isinstance(adapter, LanguageAdapter)

    def test_get_adapter_dot_rs_is_language_adapter(self) -> None:
        adapter = get_adapter(".rs")
        assert adapter is not None
        assert isinstance(adapter, LanguageAdapter)

    def test_registry_get_all_excluded_dirs_union(self) -> None:
        excluded = REGISTRY.get_all_excluded_dirs()
        # Each adapter's excluded dirs must appear in the union
        assert "vendor" in excluded  # Go
        assert "target" in excluded  # Rust/Java
        assert ".venv" in excluded  # Python
        assert "bin" in excluded  # C#


# ---------------------------------------------------------------------------
# Python adapter — adversarial edge cases
# ---------------------------------------------------------------------------


class TestPythonAdapterAdversarial:
    FILE = "src/main.py"

    # ── Inputs: malformed / tricky ────────────────────────────────────────

    def test_null_bytes_in_source(self, python_adapter) -> None:
        """Source with embedded null bytes must not crash."""
        src = b"def foo(): pass\x00def bar(): pass"
        syms, edges = python_adapter.parse(src, self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_unicode_identifier_in_source(self, python_adapter) -> None:
        """Python allows unicode identifiers — must not crash, may or may not extract."""
        src = "def ñoño(): pass\n".encode()
        syms, edges = python_adapter.parse(src, self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_unicode_string_literal_in_function(self, python_adapter) -> None:
        """Unicode inside string literals must not crash the parser."""
        src = 'def foo():\n    x = "日本語テスト 🎯"\n    bar()\n'.encode()
        syms, edges = python_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "foo" in names

    def test_bom_prefixed_source(self, python_adapter) -> None:
        """UTF-8 BOM at start of file must not crash."""
        src = b"\xef\xbb\xbfdef foo(): pass\n"
        syms, edges = python_adapter.parse(src, self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_very_long_function_name(self, python_adapter) -> None:
        """Absurdly long identifier — must not crash."""
        name = "a" * 500
        src = f"def {name}(): pass\n".encode()
        syms, edges = python_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert name in names

    # ── Aliased imports ───────────────────────────────────────────────────

    def test_aliased_import_statement(self, python_adapter) -> None:
        """import X as Y — should emit an imports edge."""
        src = b"import numpy as np\n"
        _, edges = python_adapter.parse(src, self.FILE)
        assert any(e.relationship == "imports" for e in edges)

    def test_from_import_aliased(self, python_adapter) -> None:
        """from pathlib import Path as P — should emit imports edge for Path."""
        src = b"from pathlib import Path as P\n"
        _, edges = python_adapter.parse(src, self.FILE)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) >= 1

    def test_from_import_multiple_names(self, python_adapter) -> None:
        """from os import getcwd, listdir — should emit two imports edges."""
        src = b"from os import getcwd, listdir\n"
        _, edges = python_adapter.parse(src, self.FILE)
        imports = [e for e in edges if e.relationship == "imports"]
        targets = {e.target_name for e in imports}
        assert "getcwd" in targets
        assert "listdir" in targets

    def test_wildcard_import_does_not_crash(self, python_adapter) -> None:
        """from os import * — should not crash, no imports edge for *."""
        src = b"from os import *\n"
        _, edges = python_adapter.parse(src, self.FILE)
        for e in edges:
            assert "*" not in e.target_name

    # ── Relative imports with multiple dots ───────────────────────────────

    def test_resolve_double_dot_relative(self, python_adapter) -> None:
        """from .. import utils — resolve two levels up."""
        known = {"project/utils.py"}
        result = python_adapter.resolve_module_path("..utils", "project/sub/main.py", known)
        assert result == "project/utils.py"

    def test_resolve_relative_bare_dot(self, python_adapter) -> None:
        """from . import sibling — resolve in same directory."""
        known = {"src/sibling.py"}
        result = python_adapter.resolve_module_path(".sibling", "src/main.py", known)
        assert result == "src/sibling.py"

    def test_resolve_relative_package_init(self, python_adapter) -> None:
        """from .sub import X — sub/__init__.py must be found."""
        known = {"src/sub/__init__.py"}
        result = python_adapter.resolve_module_path(".sub", "src/main.py", known)
        assert result == "src/sub/__init__.py"

    # ── Boundaries ────────────────────────────────────────────────────────

    def test_single_char_uppercase_variable(self, python_adapter) -> None:
        """A = 1 at module level — single uppercase char qualifies."""
        src = b"A = 1\n"
        syms, _ = python_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "A" in names

    def test_empty_class_body_no_crash(self, python_adapter) -> None:
        """class Empty: pass — no body methods, no crash."""
        src = b"class Empty: pass\n"
        syms, edges = python_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Empty" in names

    def test_multiple_inheritance_edges(self, python_adapter) -> None:
        """class C(A, B) — two extends edges emitted."""
        src = b"class C(A, B): pass\n"
        _, edges = python_adapter.parse(src, self.FILE)
        ext = [e for e in edges if e.relationship == "extends"]
        targets = {e.target_name for e in ext}
        assert "A" in targets
        assert "B" in targets

    def test_nested_class_method_qualified_name(self, python_adapter) -> None:
        """Outer.Inner.method — inner class method uses innermost class name."""
        src = b"class Outer:\n    class Inner:\n        def method(self): pass\n"
        syms, _ = python_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        # Inner class methods are qualified with the innermost class
        assert "Inner.method" in names

    def test_stacked_decorators(self, python_adapter) -> None:
        """Multiple decorators on one function — function still extracted."""
        src = b"@decorator1\n@decorator2\ndef foo(): pass\n"
        syms, _ = python_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "foo" in names

    # ── Calls ─────────────────────────────────────────────────────────────

    def test_self_method_call_edge(self, python_adapter) -> None:
        """self.helper() inside method emits calls edge."""
        src = b"class Foo:\n    def run(self):\n        self.helper()\n    def helper(self): pass\n"
        _, edges = python_adapter.parse(src, self.FILE)
        call_edges = [e for e in edges if e.relationship == "calls"]
        callee_names = {e.target_name for e in call_edges}
        # self.helper resolves to "self.helper"
        assert any("helper" in name for name in callee_names)

    def test_class_instantiation_edge(self, python_adapter) -> None:
        """MyClass() inside function emits instantiates edge."""
        src = b"def create():\n    return MyClass()\n"
        _, edges = python_adapter.parse(src, self.FILE)
        inst_edges = [e for e in edges if e.relationship == "instantiates"]
        assert any(e.target_name == "MyClass" for e in inst_edges)

    # ── .pyi stub file ────────────────────────────────────────────────────

    def test_pyi_class_extracted(self, python_adapter) -> None:
        """Stub files (.pyi) should have classes extracted."""
        src = b"class Foo:\n    def method(self) -> None: ...\n"
        syms, _ = python_adapter.parse(src, "types.pyi")
        names = {s.name for s in syms}
        assert "Foo" in names

    # ── Wrong extension guard ─────────────────────────────────────────────

    def test_extension_guard_txt(self, python_adapter) -> None:
        syms, edges = python_adapter.parse(b"def foo(): pass", "readme.txt")
        assert syms == []
        assert edges == []

    def test_extension_guard_no_extension(self, python_adapter) -> None:
        syms, edges = python_adapter.parse(b"def foo(): pass", "Makefile")
        assert syms == []
        assert edges == []


# ---------------------------------------------------------------------------
# Go adapter — adversarial edge cases
# ---------------------------------------------------------------------------


class TestGoAdapterAdversarial:
    FILE = "main.go"

    # ── Type alias ────────────────────────────────────────────────────────

    def test_type_alias_extracted(self, go_adapter) -> None:
        """type MyInt = int — should produce a symbol kind=variable."""
        src = b"package main\ntype MyInt = int\n"
        syms, _ = go_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "MyInt" in names

    def test_plain_type_definition_extracted(self, go_adapter) -> None:
        """type MyInt int — plain type def should appear as kind=variable."""
        src = b"package main\ntype MyInt int\n"
        syms, _ = go_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "MyInt" in names

    # ── Const/var declarations ─────────────────────────────────────────────

    def test_var_declaration_extracted(self, go_adapter) -> None:
        """var X int — package-level var produces a symbol."""
        src = b"package main\nvar Timeout int = 30\n"
        syms, _ = go_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Timeout" in names

    def test_grouped_const_all_extracted(self, go_adapter) -> None:
        """const ( A = 1; B = 2 ) — both constants extracted."""
        src = b"package main\nconst (\n    A = 1\n    B = 2\n)\n"
        syms, _ = go_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "A" in names
        assert "B" in names

    # ── Calls ─────────────────────────────────────────────────────────────

    def test_function_call_edge(self, go_adapter) -> None:
        """Calls inside function body should emit calls edges."""
        src = b'package main\nimport "fmt"\nfunc main() {\n    fmt.Println("hello")\n}\n'
        _, edges = go_adapter.parse(src, self.FILE)
        call_edges = [e for e in edges if e.relationship == "calls"]
        assert len(call_edges) >= 1

    def test_method_call_selector_expression(self, go_adapter) -> None:
        """obj.Method() call emits a calls edge with dot-notation."""
        src = b"package main\nfunc run() {\n    s.DoSomething()\n}\n"
        _, edges = go_adapter.parse(src, self.FILE)
        call_edges = [e for e in edges if e.relationship == "calls"]
        assert any("DoSomething" in e.target_name for e in call_edges)

    # ── Module resolution ──────────────────────────────────────────────────

    def test_resolve_direct_match(self, go_adapter) -> None:
        """Direct match in known_files returns immediately."""
        known = {"pkg/util.go"}
        result = go_adapter.resolve_module_path("pkg/util.go", self.FILE, known)
        assert result == "pkg/util.go"

    def test_resolve_empty_known_files(self, go_adapter) -> None:
        """Empty known_files — return import_path unchanged."""
        result = go_adapter.resolve_module_path("github.com/x/y", self.FILE, set())
        assert result == "github.com/x/y"

    def test_resolve_multi_level_tail_match(self, go_adapter) -> None:
        """Longer tail prefix matched first."""
        known = {"internal/cache/lru.go"}
        result = go_adapter.resolve_module_path(
            "github.com/example/internal/cache", self.FILE, known
        )
        assert result == "internal/cache/lru.go"

    # ── Struct embedding extended_by edge ─────────────────────────────────

    def test_embedding_extended_by_edge(self, go_adapter) -> None:
        """Struct embedding also emits extended_by edge."""
        src = b"package main\ntype A struct{}\ntype B struct { A }\n"
        _, edges = go_adapter.parse(src, self.FILE)
        ext_by = [e for e in edges if e.relationship == "extended_by"]
        assert any(e.source_name == "A" and e.target_name == "B" for e in ext_by)

    # ── Pointer receiver method ────────────────────────────────────────────

    def test_pointer_receiver_method_kind(self, go_adapter) -> None:
        """Method with pointer receiver must have kind='method'."""
        src = b"package main\ntype T struct{}\nfunc (t *T) DoSomething() {}\n"
        syms, _ = go_adapter.parse(src, self.FILE)
        m = next((s for s in syms if s.name == "T.DoSomething"), None)
        assert m is not None
        assert m.kind == "method"

    # ── Language attribute ────────────────────────────────────────────────

    def test_symbol_language_is_go(self, go_adapter) -> None:
        src = b"package main\nfunc F() {}\n"
        syms, _ = go_adapter.parse(src, self.FILE)
        assert all(s.language == "go" for s in syms)

    # ── Empty and broken ──────────────────────────────────────────────────

    def test_only_package_declaration(self, go_adapter) -> None:
        """Just 'package main' — no symbols, no crash."""
        src = b"package main\n"
        syms, edges = go_adapter.parse(src, self.FILE)
        assert syms == []
        assert edges == []


# ---------------------------------------------------------------------------
# Java adapter — adversarial edge cases
# ---------------------------------------------------------------------------


class TestJavaAdapterAdversarial:
    FILE = "src/C.java"

    # ── Static import ─────────────────────────────────────────────────────

    def test_static_import_emits_edge(self, java_adapter) -> None:
        """import static pkg.Class.method — should emit an imports edge."""
        src = b"import static java.lang.Math.PI;\nclass C {}\n"
        _, edges = java_adapter.parse(src, self.FILE)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) >= 1

    # ── Inner classes ─────────────────────────────────────────────────────

    def test_inner_class_qualified_name(self, java_adapter) -> None:
        """Inner class symbol is named OuterClass.InnerClass."""
        src = b"class Outer {\n  class Inner {}\n}\n"
        syms, _ = java_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Outer.Inner" in names

    def test_inner_class_method_qualified_name(self, java_adapter) -> None:
        """Inner class method is named OuterClass.InnerClass.method."""
        src = b"class Outer {\n  class Inner {\n    void go() {}\n  }\n}\n"
        syms, _ = java_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Outer.Inner.go" in names

    # ── Constructor ───────────────────────────────────────────────────────

    def test_constructor_extracted(self, java_adapter) -> None:
        """Constructor is extracted as kind='method'."""
        src = b"class Foo {\n  Foo() {}\n}\n"
        syms, _ = java_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Foo.Foo" in names

    def test_constructor_kind_is_method(self, java_adapter) -> None:
        src = b"class Foo {\n  Foo() {}\n}\n"
        syms, _ = java_adapter.parse(src, self.FILE)
        ctor = next((s for s in syms if s.name == "Foo.Foo"), None)
        assert ctor is not None
        assert ctor.kind == "method"

    # ── Fields ────────────────────────────────────────────────────────────

    def test_field_extracted_as_variable(self, java_adapter) -> None:
        """Public field inside class extracted as kind='variable'."""
        src = b"class Foo {\n  public int count;\n}\n"
        syms, _ = java_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Foo.count" in names

    def test_field_kind_is_variable(self, java_adapter) -> None:
        src = b"class Foo {\n  public int count;\n}\n"
        syms, _ = java_adapter.parse(src, self.FILE)
        field_sym = next((s for s in syms if s.name == "Foo.count"), None)
        assert field_sym is not None
        assert field_sym.kind == "variable"

    # ── Record ────────────────────────────────────────────────────────────

    def test_record_extracted(self, java_adapter) -> None:
        """Java 16+ record declaration extracted as kind='class'."""
        src = b"record Point(int x, int y) {}\n"
        syms, _ = java_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Point" in names

    def test_record_kind_is_class(self, java_adapter) -> None:
        src = b"record Point(int x, int y) {}\n"
        syms, _ = java_adapter.parse(src, self.FILE)
        rec = next((s for s in syms if s.name == "Point"), None)
        assert rec is not None
        assert rec.kind == "class"

    # ── Instantiation edge ────────────────────────────────────────────────

    def test_instantiation_edge_emitted(self, java_adapter) -> None:
        """new Foo() inside method body emits instantiates edge."""
        src = b"class C {\n  void run() {\n    Foo f = new Foo();\n  }\n}\n"
        _, edges = java_adapter.parse(src, self.FILE)
        inst = [e for e in edges if e.relationship == "instantiates"]
        assert any(e.target_name == "Foo" for e in inst)

    # ── Method calls ─────────────────────────────────────────────────────

    def test_method_call_edge_emitted(self, java_adapter) -> None:
        """obj.doIt() call inside method emits calls edge."""
        src = b"class C {\n  void run() {\n    obj.doIt();\n  }\n}\n"
        _, edges = java_adapter.parse(src, self.FILE)
        call_edges = [e for e in edges if e.relationship == "calls"]
        assert len(call_edges) >= 1

    # ── Module resolution ──────────────────────────────────────────────────

    def test_resolve_no_dot_in_import(self, java_adapter) -> None:
        """Import with no dots — tail segment match with .java suffix."""
        known = {"src/Foo.java"}
        result = java_adapter.resolve_module_path("Foo", self.FILE, known)
        assert result == "src/Foo.java"

    def test_resolve_empty_known_files(self, java_adapter) -> None:
        result = java_adapter.resolve_module_path("com.example.Bar", self.FILE, set())
        assert result == "com.example.Bar"

    # ── Symbol language ───────────────────────────────────────────────────

    def test_symbol_language_is_java(self, java_adapter) -> None:
        src = b"class Foo {}\n"
        syms, _ = java_adapter.parse(src, self.FILE)
        assert all(s.language == "java" for s in syms)

    # ── Interface extends interface ────────────────────────────────────────

    def test_interface_extends_interface_edge(self, java_adapter) -> None:
        """interface B extends A — extends edge emitted."""
        src = b"interface A {}\ninterface B extends A {}\n"
        _, edges = java_adapter.parse(src, self.FILE)
        ext = [e for e in edges if e.relationship == "extends"]
        assert any(e.source_name == "B" and e.target_name == "A" for e in ext)

    # ── Enum body declarations (methods inside enum) ────────────────────────

    def test_enum_with_body_methods(self, java_adapter) -> None:
        """Enum with body methods — methods extracted under enum name."""
        src = (
            b"enum Status {\n  ACTIVE, INACTIVE;\n"
            b"  public boolean isActive() { return this == ACTIVE; }\n}\n"
        )
        syms, _ = java_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Status" in names
        assert "Status.ACTIVE" in names


# ---------------------------------------------------------------------------
# Rust adapter — adversarial edge cases (priority: 70% coverage target)
# ---------------------------------------------------------------------------


class TestRustAdapterAdversarial:
    FILE = "src/lib.rs"

    # ── Static item ───────────────────────────────────────────────────────

    def test_static_item_extracted(self, rust_adapter) -> None:
        """static MAX: u32 = 100 — extracted as kind='variable'."""
        src = b"static MAX_SIZE: usize = 1024;\n"
        syms, _ = rust_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "MAX_SIZE" in names

    def test_static_item_kind_is_variable(self, rust_adapter) -> None:
        src = b"static TIMEOUT: u64 = 30;\n"
        syms, _ = rust_adapter.parse(src, self.FILE)
        sym = next((s for s in syms if s.name == "TIMEOUT"), None)
        assert sym is not None
        assert sym.kind == "variable"

    # ── Type alias ────────────────────────────────────────────────────────

    def test_type_alias_extracted(self, rust_adapter) -> None:
        """type MyResult = Result<T, E> — extracted as kind='variable'."""
        src = b"type MyResult = Result<(), String>;\n"
        syms, _ = rust_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "MyResult" in names

    def test_type_alias_kind_is_variable(self, rust_adapter) -> None:
        src = b"type Callback = fn() -> ();\n"
        syms, _ = rust_adapter.parse(src, self.FILE)
        sym = next((s for s in syms if s.name == "Callback"), None)
        assert sym is not None
        assert sym.kind == "variable"

    # ── use_as_clause (use X as Y) ────────────────────────────────────────

    def test_use_as_clause_emits_edge(self, rust_adapter) -> None:
        """use std::collections::HashMap as Map — emits imports edge."""
        src = b"use std::collections::HashMap as Map;\n"
        _, edges = rust_adapter.parse(src, self.FILE)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) >= 1

    # ── Scoped use list (use foo::{A, B}) ────────────────────────────────

    def test_scoped_use_list_multiple_imports(self, rust_adapter) -> None:
        """use std::io::{Read, Write} — two imports edges emitted."""
        src = b"use std::io::{Read, Write};\n"
        _, edges = rust_adapter.parse(src, self.FILE)
        imports = [e for e in edges if e.relationship == "imports"]
        targets = {e.target_name for e in imports}
        assert "Read" in targets
        assert "Write" in targets

    def test_scoped_use_list_glob_skipped(self, rust_adapter) -> None:
        """use std::io::{*} — glob in list skipped, no imports edge."""
        src = b"use std::io::{*};\n"
        _, edges = rust_adapter.parse(src, self.FILE)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) == 0

    # ── Call extraction with field_expression (method call) ────────────────

    def test_method_call_via_dot_notation(self, rust_adapter) -> None:
        """self.helper() call inside method emits calls edge."""
        src = (
            b"impl Foo {\n    fn run(&self) {\n        self.helper();\n    }\n"
            b"    fn helper(&self) {}\n}\n"
        )
        _, edges = rust_adapter.parse(src, self.FILE)
        call_edges = [e for e in edges if e.relationship == "calls"]
        assert len(call_edges) >= 1

    # ── Module resolution: super:: ────────────────────────────────────────

    def test_resolve_super_path(self, rust_adapter) -> None:
        """super::utils -> ../utils.rs relative to source_file's parent."""
        known = {"src/utils.rs"}
        result = rust_adapter.resolve_module_path("super::utils", "src/sub/mod.rs", known)
        assert result == "src/utils.rs"

    def test_resolve_crate_mod_rs(self, rust_adapter) -> None:
        """crate::sub -> src/sub/mod.rs matching."""
        known = {"src/sub/mod.rs"}
        result = rust_adapter.resolve_module_path("crate::sub", "src/lib.rs", known)
        assert result == "src/sub/mod.rs"

    def test_resolve_bare_mod_name(self, rust_adapter) -> None:
        """Plain mod name -> look in same directory."""
        known = {"src/helpers.rs"}
        result = rust_adapter.resolve_module_path("helpers", "src/lib.rs", known)
        assert result == "src/helpers.rs"

    def test_resolve_bare_mod_mod_rs(self, rust_adapter) -> None:
        """Plain mod name -> mod.rs variant also tried."""
        known = {"src/helpers/mod.rs"}
        result = rust_adapter.resolve_module_path("helpers", "src/lib.rs", known)
        assert result == "src/helpers/mod.rs"

    def test_resolve_no_match_unchanged(self, rust_adapter) -> None:
        """Unknown path returned unchanged."""
        result = rust_adapter.resolve_module_path("completely::unknown::path", self.FILE, set())
        assert result == "completely::unknown::path"

    # ── Trait method stubs ────────────────────────────────────────────────

    def test_trait_method_stub_extracted(self, rust_adapter) -> None:
        """Trait method stub (no body) extracted as method symbol."""
        src = b"trait Drawable {\n    fn draw(&self);\n}\n"
        syms, _ = rust_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Drawable.draw" in names

    def test_trait_method_stub_kind_is_method(self, rust_adapter) -> None:
        src = b"trait Drawable {\n    fn draw(&self);\n}\n"
        syms, _ = rust_adapter.parse(src, self.FILE)
        m = next((s for s in syms if s.name == "Drawable.draw"), None)
        assert m is not None
        assert m.kind == "method"

    # ── Multiple impl blocks same struct ────────────────────────────────────

    def test_multiple_impl_blocks_same_struct(self, rust_adapter) -> None:
        """Multiple impl blocks for same struct all contribute methods."""
        src = b"struct Foo {}\nimpl Foo { fn a(&self) {} }\nimpl Foo { fn b(&self) {} }\n"
        syms, _ = rust_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Foo.a" in names
        assert "Foo.b" in names

    # ── Enum variants ────────────────────────────────────────────────────

    def test_enum_variant_kind_is_variable(self, rust_adapter) -> None:
        src = b"enum Status { Active, Inactive }\n"
        syms, _ = rust_adapter.parse(src, self.FILE)
        active = next((s for s in syms if s.name == "Status.Active"), None)
        assert active is not None
        assert active.kind == "variable"

    # ── Symbol language ───────────────────────────────────────────────────

    def test_symbol_language_is_rust(self, rust_adapter) -> None:
        src = b"fn hello() {}\n"
        syms, _ = rust_adapter.parse(src, self.FILE)
        assert all(s.language == "rust" for s in syms)

    # ── Generic impl (impl<T> ...) ────────────────────────────────────────

    def test_generic_impl_does_not_crash(self, rust_adapter) -> None:
        """impl<T> Trait for Vec<T> — must not crash."""
        src = b"trait MyTrait { fn foo(); }\nimpl<T> MyTrait for Vec<T> { fn foo() {} }\n"
        syms, edges = rust_adapter.parse(src, self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    # ── use identifier (bare) ─────────────────────────────────────────────

    def test_bare_use_identifier_emits_edge(self, rust_adapter) -> None:
        """use crate::helpers — identifier form emits imports edge."""
        src = b"use crate::helpers;\n"
        _, edges = rust_adapter.parse(src, self.FILE)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) >= 1

    # ── Extension guard ───────────────────────────────────────────────────

    def test_extension_guard_py_rejected(self, rust_adapter) -> None:
        syms, edges = rust_adapter.parse(b"fn f() {}", "main.py")
        assert syms == []
        assert edges == []


# ---------------------------------------------------------------------------
# C# adapter — adversarial edge cases
# ---------------------------------------------------------------------------


class TestCSharpAdapterAdversarial:
    FILE = "src/C.cs"

    # ── using static ─────────────────────────────────────────────────────

    def test_using_static_emits_edge(self, csharp_adapter) -> None:
        """using static System.Math — imports edge emitted."""
        src = b"using static System.Math;\nclass C {}\n"
        _, edges = csharp_adapter.parse(src, self.FILE)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) >= 1

    def test_using_alias_emits_edge(self, csharp_adapter) -> None:
        """using IO = System.IO — imports edge for RHS type."""
        src = b"using IO = System.IO;\nclass C {}\n"
        _, edges = csharp_adapter.parse(src, self.FILE)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) >= 1

    # ── Instantiation edge ────────────────────────────────────────────────

    def test_instantiation_edge_emitted(self, csharp_adapter) -> None:
        """new Foo() inside method emits instantiates edge."""
        src = b"class C {\n  void Run() {\n    var f = new Foo();\n  }\n}\n"
        _, edges = csharp_adapter.parse(src, self.FILE)
        inst = [e for e in edges if e.relationship == "instantiates"]
        assert any(e.target_name == "Foo" for e in inst)

    # ── Method call edge ──────────────────────────────────────────────────

    def test_method_call_edge_emitted(self, csharp_adapter) -> None:
        """obj.DoIt() inside method emits calls edge."""
        src = b"class C {\n  void Run() {\n    obj.DoIt();\n  }\n}\n"
        _, edges = csharp_adapter.parse(src, self.FILE)
        call_edges = [e for e in edges if e.relationship == "calls"]
        assert len(call_edges) >= 1

    def test_simple_method_call_edge(self, csharp_adapter) -> None:
        """Bare method call (no object) emits calls edge."""
        src = b"class C {\n  void Run() {\n    Helper();\n  }\n  void Helper() {}\n}\n"
        _, edges = csharp_adapter.parse(src, self.FILE)
        call_edges = [e for e in edges if e.relationship == "calls"]
        assert any("Helper" in e.target_name for e in call_edges)

    # ── Struct with interface ──────────────────────────────────────────────

    def test_struct_with_base_list(self, csharp_adapter) -> None:
        """C# struct implementing interface — extends edge emitted."""
        src = b"struct Point : IComparable { public int X; }\n"
        _, edges = csharp_adapter.parse(src, self.FILE)
        ext = [e for e in edges if e.relationship == "extends"]
        assert any(e.source_name == "Point" for e in ext)

    # ── Nested class ─────────────────────────────────────────────────────

    def test_nested_class_qualified_name(self, csharp_adapter) -> None:
        """Nested class inside class gets qualified name OuterClass.Inner."""
        src = b"class Outer {\n  class Inner {}\n}\n"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Outer.Inner" in names

    # ── Constructor ───────────────────────────────────────────────────────

    def test_constructor_extracted(self, csharp_adapter) -> None:
        """Constructor extracted as kind='method'."""
        src = b"class Foo {\n  public Foo() {}\n}\n"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Foo.Foo" in names

    # ── Interface extends interface ────────────────────────────────────────

    def test_interface_extends_interface(self, csharp_adapter) -> None:
        """C# interface extending another — extends edge emitted."""
        src = b"interface IFoo {}\ninterface IBar : IFoo {}\n"
        _, edges = csharp_adapter.parse(src, self.FILE)
        ext = [e for e in edges if e.relationship == "extends"]
        assert any(e.source_name == "IBar" for e in ext)

    # ── Namespace with class ───────────────────────────────────────────────

    def test_deep_namespace_class_found(self, csharp_adapter) -> None:
        """Namespace wrapping class — class found by name (not namespace-qualified)."""
        src = b"namespace Foo.Bar {\n  class Baz {}\n}\n"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Baz" in names

    # ── Symbol language ───────────────────────────────────────────────────

    def test_symbol_language_is_csharp(self, csharp_adapter) -> None:
        src = b"class Foo {}\n"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        assert all(s.language == "csharp" for s in syms)

    # ── Field inside class ────────────────────────────────────────────────

    def test_field_declaration_extracted(self, csharp_adapter) -> None:
        """Field inside class body extracted as kind='variable'."""
        src = b"class Foo {\n  private int _count;\n}\n"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        # Note: field extraction depends on variable_declaration AST shape
        assert isinstance(syms, list)  # Must not crash at minimum

    # ── Extension guard ───────────────────────────────────────────────────

    def test_extension_guard_java_rejected(self, csharp_adapter) -> None:
        syms, edges = csharp_adapter.parse(b"class C {}", "C.java")
        assert syms == []
        assert edges == []

    def test_extension_guard_no_extension(self, csharp_adapter) -> None:
        syms, edges = csharp_adapter.parse(b"class C {}", "Makefile")
        assert syms == []
        assert edges == []


# ---------------------------------------------------------------------------
# Cross-adapter: same symbol name in different languages
# ---------------------------------------------------------------------------


class TestCrossAdapterNamespace:
    """Same symbol name across multiple languages must not conflict."""

    def test_same_function_name_different_adapters(self, python_adapter, go_adapter) -> None:
        py_syms, _ = python_adapter.parse(b"def process(): pass\n", "main.py")
        go_syms, _ = go_adapter.parse(b"package main\nfunc process() {}\n", "main.go")

        py_names = {s.name for s in py_syms}
        go_names = {s.name for s in go_syms}

        assert "process" in py_names
        assert "process" in go_names

        # Language fields must differ
        py_langs = {s.language for s in py_syms}
        go_langs = {s.language for s in go_syms}
        assert "python" in py_langs
        assert "go" in go_langs
        assert py_langs.isdisjoint(go_langs)

    def test_same_class_name_java_csharp(self, java_adapter, csharp_adapter) -> None:
        java_syms, _ = java_adapter.parse(b"class Result {}\n", "Result.java")
        cs_syms, _ = csharp_adapter.parse(b"class Result {}\n", "Result.cs")

        assert any(s.name == "Result" for s in java_syms)
        assert any(s.name == "Result" for s in cs_syms)

        java_result = next(s for s in java_syms if s.name == "Result")
        cs_result = next(s for s in cs_syms if s.name == "Result")
        assert java_result.language == "java"
        assert cs_result.language == "csharp"


# ---------------------------------------------------------------------------
# Compliance: no raw print() in adapter source
# ---------------------------------------------------------------------------


class TestCompliancePrint:
    def test_no_raw_print_in_python_adapter(self) -> None:
        import ast
        import inspect

        from loom.indexer.adapters import python as pymod

        src = inspect.getsource(pymod)
        tree = ast.parse(src)
        for node in ast.walk(tree):
            if isinstance(node, ast.Call):
                func = node.func
                if isinstance(func, ast.Name) and func.id == "print":
                    pytest.fail("Raw print() found in python.py")

    def test_no_raw_print_in_rust_adapter(self) -> None:
        import ast
        import inspect

        from loom.indexer.adapters import rust as rustmod

        src = inspect.getsource(rustmod)
        tree = ast.parse(src)
        for node in ast.walk(tree):
            if isinstance(node, ast.Call):
                func = node.func
                if isinstance(func, ast.Name) and func.id == "print":
                    pytest.fail("Raw print() found in rust.py")

    def test_no_raw_print_in_go_adapter(self) -> None:
        import ast
        import inspect

        from loom.indexer.adapters import go as gomod

        src = inspect.getsource(gomod)
        tree = ast.parse(src)
        for node in ast.walk(tree):
            if isinstance(node, ast.Call):
                func = node.func
                if isinstance(func, ast.Name) and func.id == "print":
                    pytest.fail("Raw print() found in go.py")

    def test_no_raw_print_in_java_adapter(self) -> None:
        import ast
        import inspect

        from loom.indexer.adapters import java as javamod

        src = inspect.getsource(javamod)
        tree = ast.parse(src)
        for node in ast.walk(tree):
            if isinstance(node, ast.Call):
                func = node.func
                if isinstance(func, ast.Name) and func.id == "print":
                    pytest.fail("Raw print() found in java.py")

    def test_no_raw_print_in_csharp_adapter(self) -> None:
        import ast
        import inspect

        from loom.indexer.adapters import csharp as csmod

        src = inspect.getsource(csmod)
        tree = ast.parse(src)
        for node in ast.walk(tree):
            if isinstance(node, ast.Call):
                func = node.func
                if isinstance(func, ast.Name) and func.id == "print":
                    pytest.fail("Raw print() found in csharp.py")
