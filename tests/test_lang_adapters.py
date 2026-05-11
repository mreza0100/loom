"""Tests for all 5 new language adapters: Python, Go, Java, Rust, C#."""

from __future__ import annotations

from pathlib import Path

import pytest

from loom.indexer.adapters import REGISTRY
from loom.indexer.adapters.base import LanguageAdapter

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def python_adapter():  # type: ignore[return]
    from loom.indexer.adapters.python import PythonAdapter

    return PythonAdapter()


@pytest.fixture
def go_adapter():  # type: ignore[return]
    from loom.indexer.adapters.go import GoAdapter

    return GoAdapter()


@pytest.fixture
def java_adapter():  # type: ignore[return]
    from loom.indexer.adapters.java import JavaAdapter

    return JavaAdapter()


@pytest.fixture
def rust_adapter():  # type: ignore[return]
    from loom.indexer.adapters.rust import RustAdapter

    return RustAdapter()


@pytest.fixture
def csharp_adapter():  # type: ignore[return]
    from loom.indexer.adapters.csharp import CSharpAdapter

    return CSharpAdapter()


# ---------------------------------------------------------------------------
# 1. Python Adapter
# ---------------------------------------------------------------------------


class TestPythonAdapter:
    SOURCE = b"def foo(): pass\nclass Bar: pass\nMAX = 1\nimport os\nfrom pathlib import Path"
    FILE = "src/main.py"

    def test_protocol_conformance(self, python_adapter) -> None:
        assert isinstance(python_adapter, LanguageAdapter)

    def test_extension_guard_wrong_ext(self, python_adapter) -> None:
        syms, edges = python_adapter.parse(b"def foo(): pass", "main.rb")
        assert syms == []
        assert edges == []

    def test_empty_source(self, python_adapter) -> None:
        syms, edges = python_adapter.parse(b"", self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_broken_source(self, python_adapter) -> None:
        syms, edges = python_adapter.parse(b"{{{{", self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_function_extracted(self, python_adapter) -> None:
        syms, _ = python_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "foo" in names

    def test_class_extracted(self, python_adapter) -> None:
        syms, _ = python_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "Bar" in names

    def test_variable_extracted(self, python_adapter) -> None:
        syms, _ = python_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "MAX" in names

    def test_function_kind(self, python_adapter) -> None:
        syms, _ = python_adapter.parse(self.SOURCE, self.FILE)
        foo = next(s for s in syms if s.name == "foo")
        assert foo.kind == "function"

    def test_class_kind(self, python_adapter) -> None:
        syms, _ = python_adapter.parse(self.SOURCE, self.FILE)
        bar = next(s for s in syms if s.name == "Bar")
        assert bar.kind == "class"

    def test_variable_kind(self, python_adapter) -> None:
        syms, _ = python_adapter.parse(self.SOURCE, self.FILE)
        var = next(s for s in syms if s.name == "MAX")
        assert var.kind == "variable"

    def test_import_edge(self, python_adapter) -> None:
        _, edges = python_adapter.parse(self.SOURCE, self.FILE)
        relationships = {e.relationship for e in edges}
        assert "imports" in relationships

    def test_import_os_edge(self, python_adapter) -> None:
        _, edges = python_adapter.parse(self.SOURCE, self.FILE)
        targets = {e.target_name for e in edges}
        assert "os" in targets

    def test_inheritance_edge(self, python_adapter) -> None:
        src = b"class Child(Parent): pass"
        _, edges = python_adapter.parse(src, self.FILE)
        ext_edges = [e for e in edges if e.relationship == "extends"]
        assert any(e.source_name == "Child" and e.target_name == "Parent" for e in ext_edges)

    def test_method_extracted(self, python_adapter) -> None:
        src = b"class Foo:\n    def bar(self): pass"
        syms, _ = python_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Foo.bar" in names

    def test_method_kind(self, python_adapter) -> None:
        src = b"class Foo:\n    def bar(self): pass"
        syms, _ = python_adapter.parse(src, self.FILE)
        method = next(s for s in syms if s.name == "Foo.bar")
        assert method.kind == "method"

    def test_decorated_function(self, python_adapter) -> None:
        src = b"@property\ndef myprop(self): return 1"
        syms, _ = python_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "myprop" in names

    def test_resolve_no_match(self, python_adapter) -> None:
        result = python_adapter.resolve_module_path("nonexistent.module", self.FILE, set())
        assert result == "nonexistent.module"

    def test_resolve_direct_match(self, python_adapter) -> None:
        known = {"src/utils.py"}
        result = python_adapter.resolve_module_path("src/utils.py", self.FILE, known)
        assert result == "src/utils.py"

    def test_resolve_dotted_module(self, python_adapter) -> None:
        known = {"foo/bar.py"}
        result = python_adapter.resolve_module_path("foo.bar", self.FILE, known)
        assert result == "foo/bar.py"

    def test_resolve_dotted_package(self, python_adapter) -> None:
        known = {"foo/bar/__init__.py"}
        result = python_adapter.resolve_module_path("foo.bar", self.FILE, known)
        assert result == "foo/bar/__init__.py"

    def test_resolve_relative_import(self, python_adapter) -> None:
        known = {"src/utils.py"}
        result = python_adapter.resolve_module_path(".utils", "src/main.py", known)
        assert result == "src/utils.py"

    def test_registry_integration(self) -> None:
        assert REGISTRY.get_adapter(".py") is not None
        assert REGISTRY.get_adapter(".pyi") is not None

    def test_config_propagation(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert ".py" in config.watch_extensions
        assert ".pyi" in config.watch_extensions

    def test_excluded_dirs_propagation(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert ".venv" in config.excluded_dirs
        assert "venv" in config.excluded_dirs
        assert ".tox" in config.excluded_dirs
        assert ".mypy_cache" in config.excluded_dirs

    def test_lowercase_var_not_extracted(self, python_adapter) -> None:
        """lowercase module-level assignments are NOT indexed."""
        src = b"x = 1\ny = 2"
        syms, _ = python_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "x" not in names
        assert "y" not in names

    def test_pyi_extension_accepted(self, python_adapter) -> None:
        src = b"def foo() -> None: ..."
        syms, _ = python_adapter.parse(src, "stubs.pyi")
        names = {s.name for s in syms}
        assert "foo" in names

    def test_calls_extracted(self, python_adapter) -> None:
        src = b"def foo():\n    bar()\n\ndef bar(): pass"
        _, edges = python_adapter.parse(src, self.FILE)
        call_edges = [e for e in edges if e.relationship == "calls"]
        assert any(e.source_name == "foo" and e.target_name == "bar" for e in call_edges)

    def test_from_import_edge(self, python_adapter) -> None:
        src = b"from pathlib import Path"
        _, edges = python_adapter.parse(src, self.FILE)
        assert any(e.relationship == "imports" and e.target_name == "Path" for e in edges)


# ---------------------------------------------------------------------------
# 2. Go Adapter
# ---------------------------------------------------------------------------


class TestGoAdapter:
    SOURCE = (
        b'package main\nimport "fmt"\nconst X = 1\nfunc F() {}\ntype S struct{}\nfunc (s S) M() {}'
    )
    FILE = "main.go"

    def test_protocol_conformance(self, go_adapter) -> None:
        assert isinstance(go_adapter, LanguageAdapter)

    def test_extension_guard_wrong_ext(self, go_adapter) -> None:
        syms, edges = go_adapter.parse(b"package main", "main.py")
        assert syms == []
        assert edges == []

    def test_empty_source(self, go_adapter) -> None:
        syms, edges = go_adapter.parse(b"", self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_broken_source(self, go_adapter) -> None:
        syms, edges = go_adapter.parse(b"{{{{", self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_function_extracted(self, go_adapter) -> None:
        syms, _ = go_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "F" in names

    def test_function_kind(self, go_adapter) -> None:
        syms, _ = go_adapter.parse(self.SOURCE, self.FILE)
        func = next(s for s in syms if s.name == "F")
        assert func.kind == "function"

    def test_struct_extracted(self, go_adapter) -> None:
        syms, _ = go_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "S" in names

    def test_struct_kind(self, go_adapter) -> None:
        syms, _ = go_adapter.parse(self.SOURCE, self.FILE)
        s = next(sym for sym in syms if sym.name == "S")
        assert s.kind == "class"

    def test_method_extracted(self, go_adapter) -> None:
        syms, _ = go_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "S.M" in names

    def test_method_kind(self, go_adapter) -> None:
        syms, _ = go_adapter.parse(self.SOURCE, self.FILE)
        m = next(s for s in syms if s.name == "S.M")
        assert m.kind == "method"

    def test_const_extracted(self, go_adapter) -> None:
        syms, _ = go_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "X" in names

    def test_import_edge(self, go_adapter) -> None:
        _, edges = go_adapter.parse(self.SOURCE, self.FILE)
        assert any(e.relationship == "imports" for e in edges)

    def test_interface_extracted(self, go_adapter) -> None:
        src = b"package main\ntype I interface { Method() string }"
        syms, _ = go_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "I" in names

    def test_interface_kind(self, go_adapter) -> None:
        src = b"package main\ntype I interface { Method() string }"
        syms, _ = go_adapter.parse(src, self.FILE)
        iface = next(s for s in syms if s.name == "I")
        assert iface.kind == "class"

    def test_embedding_edge(self, go_adapter) -> None:
        src = b"package main\ntype A struct{}\ntype B struct { A }"
        _, edges = go_adapter.parse(src, self.FILE)
        ext_edges = [e for e in edges if e.relationship == "extends"]
        assert any(e.source_name == "B" and e.target_name == "A" for e in ext_edges)

    def test_resolve_no_match(self, go_adapter) -> None:
        result = go_adapter.resolve_module_path("github.com/example/pkg", self.FILE, set())
        assert result == "github.com/example/pkg"

    def test_resolve_tail_match(self, go_adapter) -> None:
        known = {"pkg/util/helper.go"}
        result = go_adapter.resolve_module_path("github.com/example/pkg/util", self.FILE, known)
        assert result == "pkg/util/helper.go"

    def test_registry_integration(self) -> None:
        assert REGISTRY.get_adapter(".go") is not None

    def test_config_propagation(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert ".go" in config.watch_extensions

    def test_excluded_dirs_propagation(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert "vendor" in config.excluded_dirs

    def test_grouped_import(self, go_adapter) -> None:
        src = b'package main\nimport (\n    "fmt"\n    "os"\n)\nfunc F() {}'
        _, edges = go_adapter.parse(src, self.FILE)
        targets = {e.target_file for e in edges if e.relationship == "imports"}
        assert "fmt" in targets
        assert "os" in targets

    def test_pointer_receiver_method(self, go_adapter) -> None:
        src = b"package main\ntype T struct{}\nfunc (t *T) DoSomething() {}"
        syms, _ = go_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "T.DoSomething" in names


# ---------------------------------------------------------------------------
# 3. Java Adapter
# ---------------------------------------------------------------------------


class TestJavaAdapter:
    SOURCE = (
        b"import com.example.Foo;\nclass C extends Base implements I {\n  public void m() {}\n}"
    )
    FILE = "src/C.java"

    def test_protocol_conformance(self, java_adapter) -> None:
        assert isinstance(java_adapter, LanguageAdapter)

    def test_extension_guard_wrong_ext(self, java_adapter) -> None:
        syms, edges = java_adapter.parse(b"class C {}", "C.py")
        assert syms == []
        assert edges == []

    def test_empty_source(self, java_adapter) -> None:
        syms, edges = java_adapter.parse(b"", self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_broken_source(self, java_adapter) -> None:
        syms, edges = java_adapter.parse(b"{{{{", self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_class_extracted(self, java_adapter) -> None:
        syms, _ = java_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "C" in names

    def test_class_kind(self, java_adapter) -> None:
        syms, _ = java_adapter.parse(self.SOURCE, self.FILE)
        c = next(s for s in syms if s.name == "C")
        assert c.kind == "class"

    def test_method_extracted(self, java_adapter) -> None:
        syms, _ = java_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "C.m" in names

    def test_method_kind(self, java_adapter) -> None:
        syms, _ = java_adapter.parse(self.SOURCE, self.FILE)
        m = next(s for s in syms if s.name == "C.m")
        assert m.kind == "method"

    def test_import_edge(self, java_adapter) -> None:
        _, edges = java_adapter.parse(self.SOURCE, self.FILE)
        assert any(e.relationship == "imports" for e in edges)

    def test_extends_edge(self, java_adapter) -> None:
        _, edges = java_adapter.parse(self.SOURCE, self.FILE)
        ext_edges = [e for e in edges if e.relationship == "extends"]
        assert any(e.source_name == "C" and e.target_name == "Base" for e in ext_edges)

    def test_implements_edge(self, java_adapter) -> None:
        _, edges = java_adapter.parse(self.SOURCE, self.FILE)
        impl_edges = [e for e in edges if e.relationship == "implements"]
        assert any(e.source_name == "C" and e.target_name == "I" for e in impl_edges)

    def test_enum_extracted(self, java_adapter) -> None:
        src = b"enum Color { RED, GREEN, BLUE }"
        syms, _ = java_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Color" in names

    def test_enum_constants_extracted(self, java_adapter) -> None:
        src = b"enum Color { RED, GREEN, BLUE }"
        syms, _ = java_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Color.RED" in names
        assert "Color.GREEN" in names

    def test_interface_extracted(self, java_adapter) -> None:
        src = b"interface Runnable { void run(); }"
        syms, _ = java_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Runnable" in names

    def test_wildcard_import_skipped(self, java_adapter) -> None:
        src = b"import java.util.*;\nclass D {}"
        _, edges = java_adapter.parse(src, self.FILE)
        # Wildcard import should be skipped, no imports edge for java.util.*
        for e in edges:
            assert "*" not in e.target_name

    def test_resolve_no_match(self, java_adapter) -> None:
        result = java_adapter.resolve_module_path("com.example.Foo", self.FILE, set())
        assert result == "com.example.Foo"

    def test_resolve_dot_to_slash(self, java_adapter) -> None:
        known = {"com/example/Foo.java"}
        result = java_adapter.resolve_module_path("com.example.Foo", self.FILE, known)
        assert result == "com/example/Foo.java"

    def test_resolve_tail_match(self, java_adapter) -> None:
        known = {"src/main/java/com/example/Foo.java"}
        result = java_adapter.resolve_module_path("com.example.Foo", self.FILE, known)
        assert result == "src/main/java/com/example/Foo.java"

    def test_registry_integration(self) -> None:
        assert REGISTRY.get_adapter(".java") is not None

    def test_config_propagation(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert ".java" in config.watch_extensions

    def test_excluded_dirs_propagation(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert "target" in config.excluded_dirs
        assert "build" in config.excluded_dirs


# ---------------------------------------------------------------------------
# 4. Rust Adapter
# ---------------------------------------------------------------------------


class TestRustAdapter:
    SOURCE = (
        b"use crate::x;\nfn f() {}\nstruct S {}\ntrait T {}"
        b"\nimpl S { fn m(&self) {} }\nimpl T for S { fn t_m(&self) {} }"
    )
    FILE = "src/lib.rs"

    def test_protocol_conformance(self, rust_adapter) -> None:
        assert isinstance(rust_adapter, LanguageAdapter)

    def test_extension_guard_wrong_ext(self, rust_adapter) -> None:
        syms, edges = rust_adapter.parse(b"fn f() {}", "main.py")
        assert syms == []
        assert edges == []

    def test_empty_source(self, rust_adapter) -> None:
        syms, edges = rust_adapter.parse(b"", self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_broken_source(self, rust_adapter) -> None:
        syms, edges = rust_adapter.parse(b"{{{{", self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_function_extracted(self, rust_adapter) -> None:
        syms, _ = rust_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "f" in names

    def test_function_kind(self, rust_adapter) -> None:
        syms, _ = rust_adapter.parse(self.SOURCE, self.FILE)
        func = next(s for s in syms if s.name == "f")
        assert func.kind == "function"

    def test_struct_extracted(self, rust_adapter) -> None:
        syms, _ = rust_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "S" in names

    def test_struct_kind(self, rust_adapter) -> None:
        syms, _ = rust_adapter.parse(self.SOURCE, self.FILE)
        s = next(sym for sym in syms if sym.name == "S")
        assert s.kind == "class"

    def test_trait_extracted(self, rust_adapter) -> None:
        syms, _ = rust_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "T" in names

    def test_trait_kind(self, rust_adapter) -> None:
        syms, _ = rust_adapter.parse(self.SOURCE, self.FILE)
        t = next(sym for sym in syms if sym.name == "T")
        assert t.kind == "class"

    def test_impl_method_extracted(self, rust_adapter) -> None:
        syms, _ = rust_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "S.m" in names

    def test_impl_method_kind(self, rust_adapter) -> None:
        syms, _ = rust_adapter.parse(self.SOURCE, self.FILE)
        m = next(s for s in syms if s.name == "S.m")
        assert m.kind == "method"

    def test_implements_edge(self, rust_adapter) -> None:
        _, edges = rust_adapter.parse(self.SOURCE, self.FILE)
        impl_edges = [e for e in edges if e.relationship == "implements"]
        assert any(e.source_name == "S" and e.target_name == "T" for e in impl_edges)

    def test_implemented_by_edge(self, rust_adapter) -> None:
        _, edges = rust_adapter.parse(self.SOURCE, self.FILE)
        impl_edges = [e for e in edges if e.relationship == "implemented_by"]
        assert any(e.source_name == "T" and e.target_name == "S" for e in impl_edges)

    def test_use_import_edge(self, rust_adapter) -> None:
        _, edges = rust_adapter.parse(self.SOURCE, self.FILE)
        assert any(e.relationship == "imports" for e in edges)

    def test_enum_extracted(self, rust_adapter) -> None:
        src = b"enum Color { Red, Green, Blue }"
        syms, _ = rust_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Color" in names

    def test_enum_variants_extracted(self, rust_adapter) -> None:
        src = b"enum Color { Red, Green, Blue }"
        syms, _ = rust_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Color.Red" in names
        assert "Color.Green" in names

    def test_const_extracted(self, rust_adapter) -> None:
        src = b"const MAX: u32 = 100;"
        syms, _ = rust_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "MAX" in names

    def test_macro_extracted(self, rust_adapter) -> None:
        src = b"macro_rules! mymacro { () => {} }"
        syms, _ = rust_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "mymacro" in names

    def test_macro_kind(self, rust_adapter) -> None:
        src = b"macro_rules! mymacro { () => {} }"
        syms, _ = rust_adapter.parse(src, self.FILE)
        macro = next(s for s in syms if s.name == "mymacro")
        assert macro.kind == "macro"

    def test_glob_use_skipped(self, rust_adapter) -> None:
        src = b"use std::collections::*;"
        _, edges = rust_adapter.parse(src, self.FILE)
        # Wildcard import should be skipped, no imports edge
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) == 0

    def test_resolve_no_match(self, rust_adapter) -> None:
        result = rust_adapter.resolve_module_path("nonexistent", self.FILE, set())
        assert result == "nonexistent"

    def test_resolve_crate_path(self, rust_adapter) -> None:
        known = {"src/utils.rs"}
        result = rust_adapter.resolve_module_path("crate::utils", self.FILE, known)
        assert result == "src/utils.rs"

    def test_registry_integration(self) -> None:
        assert REGISTRY.get_adapter(".rs") is not None

    def test_config_propagation(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert ".rs" in config.watch_extensions

    def test_excluded_dirs_propagation(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert "target" in config.excluded_dirs

    def test_trait_impl_method_extracted(self, rust_adapter) -> None:
        """Methods from `impl Trait for Struct` should be named Struct.method."""
        syms, _ = rust_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "S.t_m" in names


# ---------------------------------------------------------------------------
# 5. C# Adapter
# ---------------------------------------------------------------------------


class TestCSharpAdapter:
    SOURCE = b"using System;\nclass C : Base {\n  public void M() {}\n  public string P { get; }\n}"
    FILE = "src/C.cs"

    def test_protocol_conformance(self, csharp_adapter) -> None:
        assert isinstance(csharp_adapter, LanguageAdapter)

    def test_extension_guard_wrong_ext(self, csharp_adapter) -> None:
        syms, edges = csharp_adapter.parse(b"class C {}", "C.java")
        assert syms == []
        assert edges == []

    def test_empty_source(self, csharp_adapter) -> None:
        syms, edges = csharp_adapter.parse(b"", self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_broken_source(self, csharp_adapter) -> None:
        syms, edges = csharp_adapter.parse(b"{{{{", self.FILE)
        assert isinstance(syms, list)
        assert isinstance(edges, list)

    def test_class_extracted(self, csharp_adapter) -> None:
        syms, _ = csharp_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "C" in names

    def test_class_kind(self, csharp_adapter) -> None:
        syms, _ = csharp_adapter.parse(self.SOURCE, self.FILE)
        c = next(s for s in syms if s.name == "C")
        assert c.kind == "class"

    def test_method_extracted(self, csharp_adapter) -> None:
        syms, _ = csharp_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "C.M" in names

    def test_method_kind(self, csharp_adapter) -> None:
        syms, _ = csharp_adapter.parse(self.SOURCE, self.FILE)
        m = next(s for s in syms if s.name == "C.M")
        assert m.kind == "method"

    def test_property_extracted(self, csharp_adapter) -> None:
        syms, _ = csharp_adapter.parse(self.SOURCE, self.FILE)
        names = {s.name for s in syms}
        assert "C.P" in names

    def test_property_kind(self, csharp_adapter) -> None:
        syms, _ = csharp_adapter.parse(self.SOURCE, self.FILE)
        p = next(s for s in syms if s.name == "C.P")
        assert p.kind == "variable"

    def test_using_edge(self, csharp_adapter) -> None:
        _, edges = csharp_adapter.parse(self.SOURCE, self.FILE)
        assert any(e.relationship == "imports" and e.target_name == "System" for e in edges)

    def test_extends_edge(self, csharp_adapter) -> None:
        _, edges = csharp_adapter.parse(self.SOURCE, self.FILE)
        ext_edges = [e for e in edges if e.relationship == "extends"]
        assert any(e.source_name == "C" and e.target_name == "Base" for e in ext_edges)

    def test_enum_extracted(self, csharp_adapter) -> None:
        src = b"enum Status { Active, Inactive }"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Status" in names

    def test_enum_members_extracted(self, csharp_adapter) -> None:
        src = b"enum Status { Active, Inactive }"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Status.Active" in names
        assert "Status.Inactive" in names

    def test_interface_extracted(self, csharp_adapter) -> None:
        src = b"interface IRunnable { void Run(); }"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "IRunnable" in names

    def test_namespace_traversal(self, csharp_adapter) -> None:
        src = b"namespace Foo {\n  class Bar {}\n}"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Bar" in names

    def test_resolve_no_match(self, csharp_adapter) -> None:
        result = csharp_adapter.resolve_module_path("System.IO", self.FILE, set())
        assert result == "System.IO"

    def test_resolve_direct_match(self, csharp_adapter) -> None:
        known = {"System.IO"}
        result = csharp_adapter.resolve_module_path("System.IO", self.FILE, known)
        assert result == "System.IO"

    def test_registry_integration(self) -> None:
        assert REGISTRY.get_adapter(".cs") is not None

    def test_config_propagation(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert ".cs" in config.watch_extensions

    def test_excluded_dirs_propagation(self, tmp_path: Path) -> None:
        from loom.config import LoomConfig

        config = LoomConfig(target_dir=tmp_path)
        assert "bin" in config.excluded_dirs
        assert "obj" in config.excluded_dirs

    def test_struct_extracted(self, csharp_adapter) -> None:
        src = b"struct Point { public int X; }"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Point" in names

    def test_record_extracted(self, csharp_adapter) -> None:
        src = b"record Person(string Name, int Age);"
        syms, _ = csharp_adapter.parse(src, self.FILE)
        names = {s.name for s in syms}
        assert "Person" in names


# ---------------------------------------------------------------------------
# 6. Cross-adapter: Language name and extensions attributes
# ---------------------------------------------------------------------------


class TestAdapterAttributes:
    def test_python_language_name(self, python_adapter) -> None:
        assert python_adapter.language_name == "python"

    def test_go_language_name(self, go_adapter) -> None:
        assert go_adapter.language_name == "go"

    def test_java_language_name(self, java_adapter) -> None:
        assert java_adapter.language_name == "java"

    def test_rust_language_name(self, rust_adapter) -> None:
        assert rust_adapter.language_name == "rust"

    def test_csharp_language_name(self, csharp_adapter) -> None:
        assert csharp_adapter.language_name == "csharp"

    def test_python_extensions_frozenset(self, python_adapter) -> None:
        assert isinstance(python_adapter.extensions, frozenset)
        assert ".py" in python_adapter.extensions

    def test_go_extensions_frozenset(self, go_adapter) -> None:
        assert isinstance(go_adapter.extensions, frozenset)
        assert ".go" in go_adapter.extensions

    def test_java_extensions_frozenset(self, java_adapter) -> None:
        assert isinstance(java_adapter.extensions, frozenset)
        assert ".java" in java_adapter.extensions

    def test_rust_extensions_frozenset(self, rust_adapter) -> None:
        assert isinstance(rust_adapter.extensions, frozenset)
        assert ".rs" in rust_adapter.extensions

    def test_csharp_extensions_frozenset(self, csharp_adapter) -> None:
        assert isinstance(csharp_adapter.extensions, frozenset)
        assert ".cs" in csharp_adapter.extensions

    def test_python_excluded_dirs_frozenset(self, python_adapter) -> None:
        assert isinstance(python_adapter.excluded_dirs, frozenset)

    def test_go_excluded_dirs_frozenset(self, go_adapter) -> None:
        assert isinstance(go_adapter.excluded_dirs, frozenset)

    def test_java_excluded_dirs_frozenset(self, java_adapter) -> None:
        assert isinstance(java_adapter.excluded_dirs, frozenset)

    def test_rust_excluded_dirs_frozenset(self, rust_adapter) -> None:
        assert isinstance(rust_adapter.excluded_dirs, frozenset)

    def test_csharp_excluded_dirs_frozenset(self, csharp_adapter) -> None:
        assert isinstance(csharp_adapter.excluded_dirs, frozenset)
