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

pub struct JavaAdapter;

impl LanguageAdapter for JavaAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &[".java"]
    }

    fn language_name(&self) -> &'static str {
        "java"
    }

    fn excluded_dirs(&self) -> &'static [&'static str] {
        &["target", "build", ".gradle", ".idea", "out"]
    }

    fn parse(&self, source: &[u8], file_path: &str) -> Result<ParseResult> {
        if !file_path.ends_with(".java") {
            return Ok(ParseResult::default());
        }
        language_parse_result(
            source,
            file_path,
            "java",
            tree_sitter_java::LANGUAGE.into(),
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
        let slash = format!("{}.java", import_path.replace('.', "/"));
        if known_files.contains(&slash) {
            return slash;
        }
        let last = import_path.rsplit('.').next().unwrap_or(import_path);
        let suffix = format!("/{last}.java");
        known_files
            .iter()
            .find(|file| file.ends_with(&suffix))
            .cloned()
            .unwrap_or_else(|| import_path.to_string())
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
        "import_declaration" => {
            handle_import(node, source, result);
            return;
        }
        "class_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "record_declaration" => {
            handle_type(node, source, file_path, result, scope);
            return;
        }
        "method_declaration" | "constructor_declaration" => {
            if let Some(container) = scope.container {
                let name_node = child_by_field(node, "name")
                    .or_else(|| first_named_child_of_kind(node, &["identifier"]));
                if let Some(name_node) = name_node {
                    let name = qualified(Some(container), &text(source, name_node));
                    result.symbols.push(symbol(
                        source,
                        node,
                        file_path,
                        "java",
                        name.clone(),
                        "method",
                    ));
                    push_call_edges(node, source, &name, &mut result.edges, &[]);
                    push_instantiates(node, source, &name, &mut result.edges);
                }
            }
            return;
        }
        "field_declaration" if scope.container.is_some() => {
            for variable in children_by_kind(node, "variable_declarator") {
                if let Some(name_node) = first_named_child_of_kind(variable, &["identifier"]) {
                    let name = qualified(scope.container, &text(source, name_node));
                    result
                        .symbols
                        .push(symbol(source, node, file_path, "java", name, "variable"));
                }
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

fn handle_import(node: Node<'_>, source: &[u8], result: &mut ParseResult) {
    if descendant_of_kind(node, &["asterisk"]).is_some() {
        return;
    }
    if let Some(import) = descendant_of_kind(node, &["scoped_identifier", "identifier"]) {
        let name = text(source, import);
        result
            .edges
            .push(edge(name.clone(), name.clone(), "imports", Some(name)));
    }
}

fn handle_type(
    node: Node<'_>,
    source: &[u8],
    file_path: &str,
    result: &mut ParseResult,
    scope: Scope<'_>,
) {
    let Some(name_node) = child_by_field(node, "name")
        .or_else(|| first_named_child_of_kind(node, &["identifier", "type_identifier"]))
    else {
        return;
    };
    let name = qualified(scope.container, &text(source, name_node));
    result.symbols.push(symbol(
        source,
        node,
        file_path,
        "java",
        name.clone(),
        "class",
    ));
    handle_bases(node, source, &name, result);
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
    for base_group in [
        "superclass",
        "super_interfaces",
        "interfaces",
        "extends_interfaces",
    ] {
        if let Some(group) = child_by_kind(node, base_group) {
            walk_preorder(group, &mut |candidate| {
                if matches!(
                    candidate.kind(),
                    "type_identifier" | "identifier" | "scoped_type_identifier"
                ) {
                    let target = text(source, candidate);
                    let relationship = match base_group {
                        "super_interfaces" | "interfaces" => "implements",
                        "superclass" | "extends_interfaces" => "extends",
                        _ => "extends",
                    };
                    result
                        .edges
                        .push(edge(name, target.clone(), relationship, None));
                    if relationship == "extends" {
                        result.edges.push(edge(target, name, "extended_by", None));
                    }
                }
            });
        }
    }
}
