use std::collections::BTreeSet;

use loom_core::{parse_file, AdapterRegistry, ParseResult};

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
