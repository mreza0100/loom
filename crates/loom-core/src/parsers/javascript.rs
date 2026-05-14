use std::{collections::BTreeSet, path::Path};

use tree_sitter::Node;

use crate::{
    models::ParsedEdge,
    parsers::{
        tree_sitter_utils::{
            child_by_field, children_by_kind, descendant_of_kind, edge, first_named_child_of_kind,
            language_parse_result, node_text, push_call_edges, push_instantiates, qualified,
            resolve_direct_or_candidates, strip_string_literal_quotes, symbol, text, walk_preorder,
            Scope,
        },
        LanguageAdapter, ParseResult,
    },
    Result,
};

pub struct JavaScriptAdapter;

impl LanguageAdapter for JavaScriptAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &[".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx"]
    }

    fn language_name(&self) -> &'static str {
        "javascript"
    }

    fn excluded_dirs(&self) -> &'static [&'static str] {
        &["node_modules", "dist", "build", ".next", "coverage"]
    }

    fn parse(&self, source: &[u8], file_path: &str) -> Result<ParseResult> {
        let extension = Path::new(file_path)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();
        let (language_name, language) = match extension {
            "js" | "jsx" | "mjs" | "cjs" => ("javascript", tree_sitter_javascript::LANGUAGE.into()),
            "ts" => (
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ),
            "tsx" => ("typescript", tree_sitter_typescript::LANGUAGE_TSX.into()),
            _ => return Ok(ParseResult::default()),
        };
        language_parse_result(
            source,
            file_path,
            language_name,
            language,
            |root, result| {
                walk_node(root, source, file_path, language_name, result, Scope::TOP);
            },
        )
    }

    fn resolve_module_path(
        &self,
        import_path: &str,
        _source_file: &str,
        known_files: &BTreeSet<String>,
    ) -> String {
        let extensions = [".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"];
        let candidates = extensions
            .iter()
            .map(|extension| format!("{import_path}{extension}"))
            .chain(["index.js", "index.ts"].map(|index| format!("{import_path}/{index}")));
        resolve_direct_or_candidates(import_path, known_files, candidates)
    }
}

fn walk_node(
    node: Node<'_>,
    source: &[u8],
    file_path: &str,
    language: &str,
    result: &mut ParseResult,
    scope: Scope<'_>,
) {
    match node.kind() {
        "import_statement" => {
            handle_import(node, source, &mut result.edges);
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
                    language,
                    name.clone(),
                    "function",
                ));
                push_call_edges(node, source, &name, &mut result.edges, &["console."]);
                push_instantiates(node, source, &name, &mut result.edges);
            }
            return;
        }
        "class_declaration" | "abstract_class_declaration" | "interface_declaration" => {
            if let Some(name_node) = child_by_field(node, "name")
                .or_else(|| first_named_child_of_kind(node, &["identifier", "type_identifier"]))
            {
                let name = text(source, name_node);
                result.symbols.push(symbol(
                    source,
                    node,
                    file_path,
                    language,
                    name.clone(),
                    "class",
                ));
                handle_heritage(node, source, &name, &mut result.edges);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    walk_node(
                        child,
                        source,
                        file_path,
                        language,
                        result,
                        Scope {
                            container: Some(&name),
                            in_function: false,
                        },
                    );
                }
            }
            return;
        }
        "type_alias_declaration" => {
            if let Some(name_node) = child_by_field(node, "name")
                .or_else(|| first_named_child_of_kind(node, &["type_identifier", "identifier"]))
            {
                let name = text(source, name_node);
                result
                    .symbols
                    .push(symbol(source, node, file_path, language, name, "variable"));
            }
            return;
        }
        "method_definition" | "method_signature" => {
            if let Some(container) = scope.container {
                if let Some(name_node) = child_by_field(node, "name").or_else(|| {
                    first_named_child_of_kind(
                        node,
                        &[
                            "property_identifier",
                            "identifier",
                            "private_property_identifier",
                        ],
                    )
                }) {
                    let name = qualified(Some(container), &text(source, name_node));
                    result.symbols.push(symbol(
                        source,
                        node,
                        file_path,
                        language,
                        name.clone(),
                        "method",
                    ));
                    push_call_edges(node, source, &name, &mut result.edges, &["console."]);
                    push_instantiates(node, source, &name, &mut result.edges);
                }
            }
            return;
        }
        "public_field_definition" | "field_definition" => {
            if let Some(container) = scope.container {
                if let Some(name_node) = child_by_field(node, "name").or_else(|| {
                    first_named_child_of_kind(
                        node,
                        &[
                            "property_identifier",
                            "identifier",
                            "private_property_identifier",
                        ],
                    )
                }) {
                    let name = qualified(Some(container), &text(source, name_node));
                    result
                        .symbols
                        .push(symbol(source, node, file_path, language, name, "variable"));
                }
            }
            return;
        }
        "lexical_declaration" | "variable_declaration" if !scope.in_function => {
            handle_variable_declaration(node, source, file_path, language, result);
        }
        "export_statement" => {}
        _ => {}
    }

    let next_scope = Scope {
        container: scope.container,
        in_function: scope.in_function
            || matches!(
                node.kind(),
                "function_declaration" | "function_expression" | "arrow_function"
            ),
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_node(child, source, file_path, language, result, next_scope);
    }
}

fn handle_variable_declaration(
    node: Node<'_>,
    source: &[u8],
    file_path: &str,
    language: &str,
    result: &mut ParseResult,
) {
    for declarator in children_by_kind(node, "variable_declarator") {
        handle_require(declarator, source, &mut result.edges);
        let Some(name_node) = child_by_field(declarator, "name")
            .or_else(|| first_named_child_of_kind(declarator, &["identifier"]))
        else {
            continue;
        };
        let name = text(source, name_node);
        let value = child_by_field(declarator, "value");
        let kind = if value
            .is_some_and(|value| matches!(value.kind(), "arrow_function" | "function_expression"))
        {
            "function"
        } else {
            "variable"
        };
        result.symbols.push(symbol(
            source,
            node,
            file_path,
            language,
            name.clone(),
            kind,
        ));
        if let Some(value) = value {
            push_call_edges(value, source, &name, &mut result.edges, &["console."]);
            push_instantiates(value, source, &name, &mut result.edges);
        }
    }
}

fn handle_import(node: Node<'_>, source: &[u8], edges: &mut Vec<ParsedEdge>) {
    let module = descendant_of_kind(node, &["string"])
        .map(|module| strip_string_literal_quotes(&text(source, module)));
    let Some(module) = module else {
        return;
    };
    let mut emitted = false;
    walk_preorder(node, &mut |candidate| match candidate.kind() {
        "import_specifier" => {
            let exported = child_by_field(candidate, "name")
                .or_else(|| first_named_child_of_kind(candidate, &["identifier"]))
                .map(|name| text(source, name));
            let local = child_by_field(candidate, "alias")
                .or_else(|| {
                    let mut names = children_by_kind(candidate, "identifier");
                    let _first = names.next();
                    names.next()
                })
                .map(|name| text(source, name));
            if let Some(exported) = exported {
                edges.push(edge(
                    local.unwrap_or_else(|| exported.clone()),
                    exported,
                    "imports",
                    Some(module.clone()),
                ));
                emitted = true;
            }
        }
        "namespace_import" | "identifier" if !emitted => {
            let name = text(source, candidate);
            if !matches!(name.as_str(), "import" | "from" | "type") {
                edges.push(edge(name.clone(), name, "imports", Some(module.clone())));
                emitted = true;
            }
        }
        _ => {}
    });
    if !emitted {
        edges.push(edge(
            module.clone(),
            module.clone(),
            "imports",
            Some(module),
        ));
    }
}

fn handle_require(node: Node<'_>, source: &[u8], edges: &mut Vec<ParsedEdge>) {
    let Some(value) = child_by_field(node, "value") else {
        return;
    };
    if !node_text(source, value).contains("require") {
        return;
    }
    let Some(module_node) = descendant_of_kind(value, &["string"]) else {
        return;
    };
    let module = strip_string_literal_quotes(&text(source, module_node));
    if let Some(name_node) = child_by_field(node, "name")
        .or_else(|| first_named_child_of_kind(node, &["identifier", "object_pattern"]))
    {
        if name_node.kind() == "object_pattern" {
            handle_destructured_require(name_node, source, &module, edges);
            return;
        }
        let name = text(source, name_node);
        edges.push(edge(name.clone(), name, "imports", Some(module)));
    }
}

fn handle_destructured_require(
    pattern: Node<'_>,
    source: &[u8],
    module: &str,
    edges: &mut Vec<ParsedEdge>,
) {
    let mut cursor = pattern.walk();
    for child in pattern.named_children(&mut cursor) {
        match child.kind() {
            "shorthand_property_identifier_pattern" => {
                let name = text(source, child);
                edges.push(edge(
                    name.clone(),
                    name,
                    "imports",
                    Some(module.to_string()),
                ));
            }
            "pair_pattern" => {
                let exported = child_by_field(child, "key")
                    .map(|key| strip_string_literal_quotes(&text(source, key)));
                let local = child_by_field(child, "value").and_then(|value| {
                    if value.kind() == "object_pattern" {
                        None
                    } else {
                        first_named_child_of_kind(
                            value,
                            &[
                                "identifier",
                                "shorthand_property_identifier_pattern",
                                "property_identifier",
                            ],
                        )
                        .or(Some(value))
                        .map(|local| text(source, local))
                    }
                });
                if let (Some(exported), Some(local)) = (exported, local) {
                    edges.push(edge(local, exported, "imports", Some(module.to_string())));
                }
            }
            "object_assignment_pattern" => {
                if let Some(name) = first_named_child_of_kind(
                    child,
                    &["shorthand_property_identifier_pattern", "identifier"],
                )
                .map(|name| text(source, name))
                {
                    edges.push(edge(
                        name.clone(),
                        name,
                        "imports",
                        Some(module.to_string()),
                    ));
                }
            }
            _ => {}
        }
    }
}

fn handle_heritage(node: Node<'_>, source: &[u8], name: &str, edges: &mut Vec<ParsedEdge>) {
    walk_preorder(node, &mut |candidate| {
        if matches!(
            candidate.kind(),
            "class_heritage" | "extends_clause" | "implements_clause"
        ) {
            for target in children_by_kind(candidate, "identifier")
                .chain(children_by_kind(candidate, "type_identifier"))
            {
                let parent = text(source, target);
                let relationship = if candidate.kind() == "implements_clause" {
                    "implements"
                } else {
                    "extends"
                };
                edges.push(edge(name, parent.clone(), relationship, None));
                if relationship == "extends" {
                    edges.push(edge(parent, name, "extended_by", None));
                }
            }
        }
    });
}
