use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::{
    parsers::{
        tree_sitter_utils::{
            child_by_field, child_by_kind, children_by_kind, descendant_of_kind, edge,
            first_named_child_of_kind, language_parse_result, push_call_edges, symbol, text,
            walk_preorder,
        },
        LanguageAdapter, ParseResult,
    },
    Result,
};

pub struct GoAdapter;

impl LanguageAdapter for GoAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &[".go"]
    }

    fn language_name(&self) -> &'static str {
        "go"
    }

    fn excluded_dirs(&self) -> &'static [&'static str] {
        &["vendor"]
    }

    fn parse(&self, source: &[u8], file_path: &str) -> Result<ParseResult> {
        if !file_path.ends_with(".go") {
            return Ok(ParseResult::default());
        }
        language_parse_result(
            source,
            file_path,
            "go",
            tree_sitter_go::LANGUAGE.into(),
            |root, result| walk_node(root, source, file_path, result),
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
        let parts: Vec<&str> = import_path.trim_matches('/').split('/').collect();
        for start in (0..parts.len()).rev() {
            let tail = parts[start..].join("/");
            if let Some(found) = known_files
                .iter()
                .find(|file| file.starts_with(&format!("{tail}/")) || **file == tail)
            {
                return found.to_string();
            }
        }
        import_path.to_string()
    }
}

fn walk_node(node: Node<'_>, source: &[u8], file_path: &str, result: &mut ParseResult) {
    match node.kind() {
        "import_declaration" => {
            handle_imports(node, source, result);
            return;
        }
        "function_declaration" => {
            if let Some(name_node) = child_by_field(node, "name")
                .or_else(|| first_named_child_of_kind(node, &["identifier"]))
            {
                let name = text(source, name_node);
                result.symbols.push(symbol(
                    source,
                    node,
                    file_path,
                    "go",
                    name.clone(),
                    "function",
                ));
                push_call_edges(node, source, &name, &mut result.edges, &[]);
            }
            return;
        }
        "method_declaration" => {
            handle_method(node, source, file_path, result);
            return;
        }
        "type_declaration" => {
            handle_type_declaration(node, source, file_path, result);
            return;
        }
        "const_declaration" | "var_declaration" => {
            for identifier in declared_identifiers_under(node, source) {
                result.symbols.push(symbol(
                    source, node, file_path, "go", identifier, "variable",
                ));
            }
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_node(child, source, file_path, result);
    }
}

fn handle_imports(node: Node<'_>, source: &[u8], result: &mut ParseResult) {
    walk_preorder(node, &mut |candidate| {
        if matches!(
            candidate.kind(),
            "interpreted_string_literal" | "raw_string_literal"
        ) {
            let path = text(source, candidate).trim_matches(['"', '`']).to_string();
            if !path.is_empty() {
                result
                    .edges
                    .push(edge(path.clone(), path.clone(), "imports", Some(path)));
            }
        }
    });
}

fn handle_method(node: Node<'_>, source: &[u8], file_path: &str, result: &mut ParseResult) {
    let receiver =
        descendant_of_kind(node, &["type_identifier"]).map(|receiver| text(source, receiver));
    let method = child_by_field(node, "name")
        .or_else(|| first_named_child_of_kind(node, &["field_identifier"]))
        .map(|name| text(source, name));
    if let (Some(receiver), Some(method)) = (receiver, method) {
        let name = format!("{receiver}.{method}");
        result.symbols.push(symbol(
            source,
            node,
            file_path,
            "go",
            name.clone(),
            "method",
        ));
        push_call_edges(node, source, &name, &mut result.edges, &[]);
    }
}

fn handle_type_declaration(
    node: Node<'_>,
    source: &[u8],
    file_path: &str,
    result: &mut ParseResult,
) {
    for spec in children_by_kind(node, "type_spec") {
        let Some(name_node) = first_named_child_of_kind(spec, &["type_identifier", "identifier"])
        else {
            continue;
        };
        let name = text(source, name_node);
        let kind = if descendant_of_kind(spec, &["interface_type"]).is_some() {
            "interface"
        } else if descendant_of_kind(spec, &["struct_type"]).is_some() {
            "class"
        } else {
            "variable"
        };
        result
            .symbols
            .push(symbol(source, spec, file_path, "go", name.clone(), kind));
        if let Some(field_declaration_list) = child_by_kind(spec, "field_declaration_list") {
            for embedded in children_by_kind(field_declaration_list, "field_declaration") {
                if child_by_kind(embedded, "field_identifier").is_none() {
                    if let Some(parent) = descendant_of_kind(embedded, &["type_identifier"]) {
                        let target = text(source, parent);
                        edges_extend(&mut result.edges, &name, &target);
                    }
                }
            }
        }
    }
}

fn declared_identifiers_under(node: Node<'_>, source: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    walk_preorder(node, &mut |candidate| {
        if matches!(candidate.kind(), "const_spec" | "var_spec") {
            let mut cursor = candidate.walk();
            for child in candidate.named_children(&mut cursor) {
                if child.kind() == "identifier" {
                    names.push(text(source, child));
                }
            }
        }
    });
    names
}

fn edges_extend(edges: &mut Vec<crate::models::ParsedEdge>, child: &str, parent: &str) {
    edges.push(edge(child, parent, "extends", None));
    edges.push(edge(parent, child, "extended_by", None));
}
