use std::collections::BTreeSet;

use loom_core::{parsers::parse_file, AdapterRegistry, ParseResult};

fn symbol_names(result: &ParseResult) -> BTreeSet<String> {
    result
        .symbols
        .iter()
        .map(|symbol| symbol.name.clone())
        .collect()
}

#[test]
fn qa_malformed_and_null_byte_sources_do_not_error() {
    let registry = AdapterRegistry::with_builtin_adapters();

    for (path, source) in [
        ("broken.rs", b"fn ok() {}\nfn broken(\0".as_slice()),
        ("broken.tsx", b"export const View = <div>{".as_slice()),
        ("broken.py", b"class Partial(\0".as_slice()),
        ("broken.go", b"package main\nfunc Broken(".as_slice()),
        ("Broken.java", b"class Broken { void m(".as_slice()),
        ("Broken.cs", b"class Broken { public void M(".as_slice()),
    ] {
        parse_file(std::path::Path::new(path), Some(source), &registry)
            .unwrap_or_else(|error| panic!("{path} should parse partially: {error}"));
    }
}

#[test]
fn qa_tsx_uses_tsx_grammar_for_jsx_and_type_parameters() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let source = br#"
import React from "react";
export const View = <T extends { id: string }>(props: T) => <div>{props.id}</div>;
"#;

    let result = parse_file(
        std::path::Path::new("component.tsx"),
        Some(source),
        &registry,
    )
    .expect("parse");

    assert!(
        symbol_names(&result).contains("View"),
        "TSX generic arrow component should be extracted as a function/variable symbol"
    );
}

#[test]
fn qa_commonjs_destructured_require_preserves_each_binding_and_alias() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let source = br#"
const { readFile, writeFile: write } = require("fs");
function load() { readFile("x", write); }
"#;

    let result =
        parse_file(std::path::Path::new("loader.js"), Some(source), &registry).expect("parse");

    assert!(
        result.edges.iter().any(|edge| {
            edge.relationship == "imports"
                && edge.source_name == "readFile"
                && edge.target_name == "readFile"
                && edge.target_file.as_deref() == Some("fs")
        }),
        "destructured require should emit an import edge for readFile"
    );
    assert!(
        result.edges.iter().any(|edge| {
            edge.relationship == "imports"
                && edge.source_name == "write"
                && edge.target_name == "writeFile"
                && edge.target_file.as_deref() == Some("fs")
        }),
        "destructured require alias should preserve local and exported names"
    );
}

#[test]
fn qa_rust_scoped_use_list_preserves_prefix_and_alias_without_prefix_noise() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let source = br#"
use crate::foo::{Bar, Baz as Renamed};
fn run() {}
"#;

    let result =
        parse_file(std::path::Path::new("lib.rs"), Some(source), &registry).expect("parse");

    assert!(
        result.edges.iter().any(|edge| {
            edge.relationship == "imports"
                && edge.source_name == "Bar"
                && edge.target_name == "Bar"
                && edge.target_file.as_deref() == Some("crate::foo::Bar")
        }),
        "scoped use list should preserve the full target path for Bar"
    );
    assert!(
        result.edges.iter().any(|edge| {
            edge.relationship == "imports"
                && edge.source_name == "Renamed"
                && edge.target_name == "Baz"
                && edge.target_file.as_deref() == Some("crate::foo::Baz")
        }),
        "scoped use alias should preserve local alias and original exported name"
    );
    assert!(
        !result.edges.iter().any(|edge| {
            edge.relationship == "imports"
                && matches!(
                    edge.target_file.as_deref(),
                    Some("crate") | Some("crate::foo")
                )
        }),
        "scoped use list should not emit intermediate path segments as imports"
    );
}

#[test]
fn qa_cross_language_empty_and_comment_only_sources_are_empty() {
    let registry = AdapterRegistry::with_builtin_adapters();

    for (path, source) in [
        ("empty.rs", b"".as_slice()),
        ("comment.py", b"# only a comment\n".as_slice()),
        ("comment.go", b"// only a comment\n".as_slice()),
        ("comment.js", b"// only a comment\n".as_slice()),
    ] {
        let result = parse_file(std::path::Path::new(path), Some(source), &registry)
            .unwrap_or_else(|error| panic!("{path} should parse: {error}"));
        assert!(
            result.symbols.is_empty(),
            "{path} should not emit symbols from empty/comment-only source"
        );
    }
}

#[test]
fn qa_java_interface_extends_uses_extends_not_implements() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let source = br#"
interface Parent {}
interface Child extends Parent {}
"#;

    let result = parse_file(
        std::path::Path::new("Interfaces.java"),
        Some(source),
        &registry,
    )
    .expect("parse");

    assert!(
        result.edges.iter().any(|edge| {
            edge.relationship == "extends"
                && edge.source_name == "Child"
                && edge.target_name == "Parent"
        }),
        "Java interface inheritance should use extends"
    );
    assert!(
        !result.edges.iter().any(|edge| {
            edge.relationship == "implements"
                && edge.source_name == "Child"
                && edge.target_name == "Parent"
        }),
        "Java interface extends must not be mislabeled as implements"
    );
}

#[test]
fn qa_go_typed_var_does_not_emit_type_as_variable() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let source = br#"
package main
type Client struct {}
var client Client
const answer int = 42
"#;

    let result =
        parse_file(std::path::Path::new("main.go"), Some(source), &registry).expect("parse");

    assert!(result
        .symbols
        .iter()
        .any(|symbol| symbol.kind == "variable" && symbol.name == "client"));
    assert!(result
        .symbols
        .iter()
        .any(|symbol| symbol.kind == "variable" && symbol.name == "answer"));
    assert!(
        !result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "variable" && symbol.name == "Client"),
        "typed var declaration should not index the type name as a variable"
    );
}

#[test]
fn qa_csharp_base_list_defaults_to_extends_without_name_heuristic() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let source = br#"
class InvoiceBase {}
class Invoice : InvoiceBase {}
class Worker : IRunnable {}
"#;

    let result =
        parse_file(std::path::Path::new("Invoice.cs"), Some(source), &registry).expect("parse");

    assert!(
        result.edges.iter().any(|edge| {
            edge.relationship == "extends"
                && edge.source_name == "Invoice"
                && edge.target_name == "InvoiceBase"
        }),
        "C# base class should be extends"
    );
    assert!(
        result.edges.iter().any(|edge| {
            edge.relationship == "extends"
                && edge.source_name == "Worker"
                && edge.target_name == "IRunnable"
        }),
        "C# parser should not use an I-prefix heuristic for implements"
    );
}

#[test]
fn qa_javascript_class_fields_are_variables_not_methods() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let source = br#"
class Widget {
  count = 0;
  render() { return this.count; }
}
"#;

    let result =
        parse_file(std::path::Path::new("widget.js"), Some(source), &registry).expect("parse");

    assert!(
        result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "variable" && symbol.name == "Widget.count"),
        "class field should be indexed as a variable"
    );
    assert!(
        !result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "method" && symbol.name == "Widget.count"),
        "class field must not be mislabeled as a method"
    );
}
