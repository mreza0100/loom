"""Tests for loom.indexer.parser — tree-sitter JS/TS parser."""

from pathlib import Path

from loom.indexer.parser import parse_file
from loom.store.models import ParsedEdge, Symbol


def _parse(code: str, filename: str = "test.js") -> tuple[list[Symbol], list[ParsedEdge]]:
    return parse_file(Path(filename), source=code.encode())


class TestFunctionDeclarations:
    def test_named_function(self) -> None:
        symbols, _ = _parse("function greet(name) { return `Hello ${name}`; }")
        assert len(symbols) == 1
        assert symbols[0].name == "greet"
        assert symbols[0].kind == "function"

    def test_arrow_function(self) -> None:
        symbols, _ = _parse("const greet = (name) => `Hello ${name}`;")
        assert len(symbols) == 1
        assert symbols[0].name == "greet"
        assert symbols[0].kind == "function"

    def test_function_expression(self) -> None:
        symbols, _ = _parse("const greet = function(name) { return name; };")
        assert len(symbols) == 1
        assert symbols[0].name == "greet"
        assert symbols[0].kind == "function"

    def test_exported_function(self) -> None:
        symbols, _ = _parse("export function greet() { }")
        assert len(symbols) == 1
        assert symbols[0].name == "greet"
        assert symbols[0].kind == "function"

    def test_exported_arrow(self) -> None:
        symbols, _ = _parse("export const greet = () => {};")
        assert len(symbols) == 1
        assert symbols[0].name == "greet"

    def test_multiple_functions(self) -> None:
        code = """
function a() {}
function b() {}
const c = () => {};
"""
        symbols, _ = _parse(code)
        names = {s.name for s in symbols}
        assert names == {"a", "b", "c"}

    def test_line_numbers(self) -> None:
        code = """
function first() {
  return 1;
}

function second() {
  return 2;
}
"""
        symbols, _ = _parse(code)
        first = next(s for s in symbols if s.name == "first")
        second = next(s for s in symbols if s.name == "second")
        assert first.line == 2
        assert second.line == 6


class TestClassDeclarations:
    def test_basic_class(self) -> None:
        code = "class MyClass { }"
        symbols, _ = _parse(code)
        assert len(symbols) == 1
        assert symbols[0].name == "MyClass"
        assert symbols[0].kind == "class"

    def test_class_with_methods(self) -> None:
        code = """
class Cart {
  addItem(item) {
    this.items.push(item);
  }

  getTotal() {
    return this.items.reduce((sum, i) => sum + i.price, 0);
  }
}
"""
        symbols, _ = _parse(code)
        names = {s.name for s in symbols}
        assert "Cart" in names
        assert "Cart.addItem" in names
        assert "Cart.getTotal" in names

        methods = [s for s in symbols if s.kind == "method"]
        assert len(methods) == 2

    def test_exported_class(self) -> None:
        symbols, _ = _parse("export class Widget { render() {} }")
        names = {s.name for s in symbols}
        assert "Widget" in names
        assert "Widget.render" in names

    def test_class_inheritance_edges(self) -> None:
        code = "class Dog extends Animal { bark() {} }"
        _, edges = _parse(code)
        extends_edges = [e for e in edges if e.relationship == "extends"]
        extended_by = [e for e in edges if e.relationship == "extended_by"]

        assert len(extends_edges) == 1
        assert extends_edges[0].source_name == "Dog"
        assert extends_edges[0].target_name == "Animal"

        assert len(extended_by) == 1
        assert extended_by[0].source_name == "Animal"
        assert extended_by[0].target_name == "Dog"


class TestVariables:
    def test_top_level_const(self) -> None:
        symbols, _ = _parse("const MAX_SIZE = 100;")
        assert len(symbols) == 1
        assert symbols[0].name == "MAX_SIZE"
        assert symbols[0].kind == "variable"

    def test_top_level_let(self) -> None:
        symbols, _ = _parse("let counter = 0;")
        assert len(symbols) == 1
        assert symbols[0].kind == "variable"

    def test_skip_inner_variables(self) -> None:
        code = """
function outer() {
  const inner = 42;
  let temp = inner + 1;
  return temp;
}
"""
        symbols, _ = _parse(code)
        names = {s.name for s in symbols}
        assert "outer" in names
        assert "inner" not in names
        assert "temp" not in names


class TestImports:
    def test_default_import(self) -> None:
        code = "import React from 'react';"
        _, edges = _parse(code)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) == 1
        assert imports[0].source_name == "React"
        assert imports[0].target_name == "React"

    def test_named_import(self) -> None:
        code = "import { useState, useEffect } from 'react';"
        _, edges = _parse(code)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) == 2
        names = {e.source_name for e in imports}
        assert "useState" in names
        assert "useEffect" in names

    def test_aliased_import(self) -> None:
        code = "import { getProduct as fetchProduct } from './product.js';"
        _, edges = _parse(code)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) == 1
        assert imports[0].source_name == "fetchProduct"
        assert imports[0].target_name == "getProduct"
        assert imports[0].target_file == "./product.js"

    def test_relative_import_path(self) -> None:
        code = "import { helper } from './utils/helper.js';"
        _, edges = _parse(code)
        imports = [e for e in edges if e.relationship == "imports"]
        assert len(imports) == 1
        assert imports[0].target_file == "./utils/helper.js"

    def test_imports_produce_parsed_edge(self) -> None:
        """Imports should produce ParsedEdge instances."""
        code = "import { foo } from './foo.js';"
        _, edges = _parse(code)
        assert all(isinstance(e, ParsedEdge) for e in edges)


class TestCallEdges:
    def test_function_call(self) -> None:
        code = """
function process() {
  validate();
  transform();
}
"""
        _, edges = _parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        targets = {e.target_name for e in calls}
        assert "validate" in targets
        assert "transform" in targets

    def test_no_self_call_edge(self) -> None:
        code = """
function recurse() {
  recurse();
}
"""
        _, edges = _parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 0

    def test_console_calls_skipped(self) -> None:
        code = """
function debug() {
  console.log("hello");
  console.error("error");
}
"""
        _, edges = _parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 0

    def test_method_call_preserves_full_expression(self) -> None:
        """Phase 3: full dotted expressions are preserved in target_name."""
        code = """
function handler() {
  db.query("SELECT 1");
}
"""
        _, edges = _parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 1
        # Full expression stored — NOT just "query"
        assert calls[0].target_name == "db.query"

    def test_new_expression(self) -> None:
        code = """
function create() {
  return new Widget();
}
"""
        _, edges = _parse(code)
        instantiates = [e for e in edges if e.relationship == "instantiates"]
        assert len(instantiates) == 1
        assert instantiates[0].target_name == "Widget"

    def test_new_no_self_instantiation(self) -> None:
        code = """
function Widget() {
  return new Widget();
}
"""
        _, edges = _parse(code)
        instantiates = [e for e in edges if e.relationship == "instantiates"]
        assert len(instantiates) == 0

    def test_full_call_expression_stored(self) -> None:
        """Phase 3: deeply dotted call expressions are stored as-is."""
        code = """
function run() {
  this.hooks.make.callAsync(options);
}
"""
        _, edges = _parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) >= 1
        target_names = {e.target_name for e in calls}
        assert "this.hooks.make.callAsync" in target_names

    def test_simple_call_unchanged(self) -> None:
        """Phase 3: simple (non-dotted) calls are unchanged."""
        code = """
function run() {
  compile();
}
"""
        _, edges = _parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 1
        assert calls[0].target_name == "compile"

    def test_method_call_on_import(self) -> None:
        """Phase 3: method calls on imported objects store full expression."""
        code = """
function readConfig() {
  return fs.readFileSync("config.json");
}
"""
        _, edges = _parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 1
        assert calls[0].target_name == "fs.readFileSync"

    def test_console_still_filtered(self) -> None:
        """Phase 3: console.* calls are still filtered even with full expression storage."""
        code = """
function logIt() {
  console.log("test");
}
"""
        _, edges = _parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 0

    def test_callee_recursion_guard(self) -> None:
        """Self-calls are filtered. foo() inside foo() produces no edge."""
        code = """
function foo() {
  foo();
}
"""
        _, edges = _parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert len(calls) == 0

    def test_calls_produce_parsed_edge(self) -> None:
        """Call edges should produce ParsedEdge instances."""
        code = """
function foo() {
  bar();
}
"""
        _, edges = _parse(code)
        calls = [e for e in edges if e.relationship == "calls"]
        assert all(isinstance(e, ParsedEdge) for e in calls)


class TestEdgeCases:
    def test_empty_file(self) -> None:
        symbols, edges = _parse("")
        assert symbols == []
        assert edges == []

    def test_comments_only(self) -> None:
        symbols, _ = _parse("// just a comment\n/* block comment */")
        assert symbols == []

    def test_unsupported_extension(self) -> None:
        # .xyz is genuinely unregistered — no adapter for it
        symbols, edges = parse_file(Path("test.xyz"), source=b"some content")
        assert symbols == []
        assert edges == []

    def test_malformed_js(self) -> None:
        code = "function { broken syntax ]]]["
        symbols, edges = _parse(code)
        assert isinstance(symbols, list)
        assert isinstance(edges, list)

    def test_context_extraction(self) -> None:
        code = "function hello() {\n  return 'world';\n}"
        symbols, _ = _parse(code)
        assert symbols[0].context.startswith("function hello()")

    def test_language_detection(self) -> None:
        symbols, _ = parse_file(Path("test.js"), source=b"function a() {}")
        assert symbols[0].language == "javascript"

        symbols, _ = parse_file(Path("test.ts"), source=b"function a() {}")
        assert symbols[0].language == "typescript"

        symbols, _ = parse_file(Path("test.tsx"), source=b"function a() {}")
        assert symbols[0].language == "typescript"

        symbols, _ = parse_file(Path("test.mjs"), source=b"function a() {}")
        assert symbols[0].language == "javascript"
