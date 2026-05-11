use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::{
    parsers::{
        tree_sitter_utils::{
            child_by_field, child_by_kind, children_by_kind, descendant_of_kind, edge,
            first_named_child_of_kind, language_parse_result, push_call_edges, push_instantiates,
            qualified, symbol, text, walk_preorder, Scope,
        },
        LanguageAdapter, ParseResult,
    },
    Result,
};

pub struct CSharpAdapter;

impl LanguageAdapter for CSharpAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &[".cs"]
    }

    fn language_name(&self) -> &'static str {
        "csharp"
    }

    fn excluded_dirs(&self) -> &'static [&'static str] {
        &["bin", "obj", ".vs", "packages"]
    }

    fn parse(&self, source: &[u8], file_path: &str) -> Result<ParseResult> {
        if !file_path.ends_with(".cs") {
            return Ok(ParseResult::default());
        }
        language_parse_result(
            source,
            file_path,
            "csharp",
            tree_sitter_c_sharp::LANGUAGE.into(),
            |root, result| walk_node(root, source, file_path, result, Scope::TOP),
        )
    }

    fn resolve_module_path(
        &self,
        import_path: &str,
        _source_file: &str,
        known_files: &BTreeSet<String>,
    ) -> String {
        if known_files.contains(import_path) {
            return import_path.to_string();
        }
        import_path.to_string()
    }
}

fn walk_node(
    node: Node<'_>,
    source: &[u8],
    file_path: &str,
    result: &mut ParseResult,
    scope: Scope<'_>,
) {
    match node.kind() {
        "using_directive" => {
            handle_using(node, source, result);
            return;
        }
        "namespace_declaration" => {}
        "class_declaration"
        | "struct_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "record_declaration" => {
            handle_type(node, source, file_path, result, scope);
            return;
        }
        "method_declaration" | "constructor_declaration" => {
            if let Some(container) = scope.container {
                if let Some(name_node) = child_by_field(node, "name")
                    .or_else(|| first_named_child_of_kind(node, &["identifier"]))
                {
                    let name = qualified(Some(container), &text(source, name_node));
                    result.symbols.push(symbol(
                        source,
                        node,
                        file_path,
                        "csharp",
                        name.clone(),
                        "method",
                    ));
                    push_call_edges(node, source, &name, &mut result.edges, &[]);
                    push_instantiates(node, source, &name, &mut result.edges);
                }
            }
            return;
        }
        "property_declaration" | "field_declaration" if scope.container.is_some() => {
            let candidates = ["variable_declarator", "identifier"];
            if let Some(name_node) = descendant_of_kind(node, &candidates) {
                let name = qualified(scope.container, &text(source, name_node));
                result
                    .symbols
                    .push(symbol(source, node, file_path, "csharp", name, "variable"));
            }
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_node(child, source, file_path, result, scope);
    }
}

fn handle_using(node: Node<'_>, source: &[u8], result: &mut ParseResult) {
    if text(source, node).contains(" = ") {
        let raw = text(source, node);
        let after = raw
            .split('=')
            .nth(1)
            .unwrap_or_default()
            .trim()
            .trim_end_matches(';');
        let before = raw.split('=').next().unwrap_or_default();
        let local = before
            .split_whitespace()
            .last()
            .unwrap_or(after)
            .to_string();
        result.edges.push(edge(
            local,
            after.to_string(),
            "imports",
            Some(after.to_string()),
        ));
        return;
    }
    for candidate in ["qualified_name", "identifier"] {
        if let Some(namespace) = descendant_of_kind(node, &[candidate]) {
            let name = text(source, namespace);
            if name != "using" && name != "static" {
                result
                    .edges
                    .push(edge(name.clone(), name.clone(), "imports", Some(name)));
                return;
            }
        }
    }
}

fn handle_type(
    node: Node<'_>,
    source: &[u8],
    file_path: &str,
    result: &mut ParseResult,
    scope: Scope<'_>,
) {
    let Some(name_node) =
        child_by_field(node, "name").or_else(|| first_named_child_of_kind(node, &["identifier"]))
    else {
        return;
    };
    let name = qualified(scope.container, &text(source, name_node));
    result.symbols.push(symbol(
        source,
        node,
        file_path,
        "csharp",
        name.clone(),
        "class",
    ));
    handle_bases(node, source, &name, result);
    if node.kind() == "enum_declaration" {
        for member in children_by_kind(node, "enum_member_declaration") {
            if let Some(member_name) = first_named_child_of_kind(member, &["identifier"]) {
                result.symbols.push(symbol(
                    source,
                    member,
                    file_path,
                    "csharp",
                    qualified(Some(&name), &text(source, member_name)),
                    "variable",
                ));
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_node(
            child,
            source,
            file_path,
            result,
            Scope {
                container: Some(&name),
                in_function: false,
            },
        );
    }
}

fn handle_bases(node: Node<'_>, source: &[u8], name: &str, result: &mut ParseResult) {
    let Some(base_list) = child_by_kind(node, "base_list") else {
        return;
    };
    walk_preorder(base_list, &mut |candidate| {
        if matches!(
            candidate.kind(),
            "identifier" | "qualified_name" | "generic_name"
        ) {
            let target = text(source, candidate);
            if !matches!(target.as_str(), ":" | ",") {
                let relationship = "extends";
                result
                    .edges
                    .push(edge(name, target.clone(), relationship, None));
                if relationship == "extends" {
                    result.edges.push(edge(target, name, "extended_by", None));
                }
            }
        }
    });
}
