use std::{collections::BTreeSet, path::Path};

use tree_sitter::Node;

use crate::{
    parsers::{
        tree_sitter_utils::{
            child_by_field, edge, first_named_child_of_kind, language_parse_result, path_parent,
            push_call_edges, qualified, resolve_direct_or_candidates, symbol, text, walk_preorder,
            Scope,
        },
        LanguageAdapter, ParseResult,
    },
    Result,
};

pub struct RustAdapter;

impl LanguageAdapter for RustAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &[".rs"]
    }

    fn language_name(&self) -> &'static str {
        "rust"
    }

    fn excluded_dirs(&self) -> &'static [&'static str] {
        &["target"]
    }

    fn parse(&self, source: &[u8], file_path: &str) -> Result<ParseResult> {
        if !file_path.ends_with(".rs") {
            return Ok(ParseResult::default());
        }
        language_parse_result(
            source,
            file_path,
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            |root, result| walk_node(root, source, file_path, result, Scope::TOP),
        )
    }

    fn resolve_module_path(
        &self,
        import_path: &str,
        source_file: &str,
        known_files: &BTreeSet<String>,
    ) -> String {
        if known_files.contains(import_path) {
            return import_path.to_string();
        }
        let source_dir = path_parent(source_file);
        let source_dir_string = source_dir.to_string_lossy();
        if let Some(remainder) = import_path.strip_prefix("crate::") {
            let path = remainder.replace("::", "/");
            let candidates = [format!("{path}.rs"), format!("{path}/mod.rs")];
            return known_files
                .iter()
                .find(|file| {
                    candidates.iter().any(|candidate| {
                        *file == candidate || file.ends_with(&format!("/{candidate}"))
                    })
                })
                .cloned()
                .unwrap_or_else(|| import_path.to_string());
        }
        if let Some(remainder) = import_path.strip_prefix("super::") {
            let parent = source_dir.parent().unwrap_or_else(|| Path::new(""));
            let path = remainder.replace("::", "/");
            return resolve_direct_or_candidates(
                import_path,
                known_files,
                [
                    parent
                        .join(format!("{path}.rs"))
                        .to_string_lossy()
                        .into_owned(),
                    parent
                        .join(format!("{path}/mod.rs"))
                        .to_string_lossy()
                        .into_owned(),
                ],
            );
        }
        let path = import_path.replace("::", "/");
        resolve_direct_or_candidates(
            import_path,
            known_files,
            [
                format!("{source_dir_string}/{path}.rs"),
                format!("{source_dir_string}/{path}/mod.rs"),
                format!("{path}.rs"),
                format!("{path}/mod.rs"),
            ],
        )
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
        "use_declaration" => {
            handle_use(node, source, result);
            return;
        }
        "function_item" => {
            if let Some(name_node) = child_by_field(node, "name")
                .or_else(|| first_named_child_of_kind(node, &["identifier"]))
            {
                let name = qualified(scope.container, &text(source, name_node));
                let kind = if scope.container.is_some() {
                    "method"
                } else {
                    "function"
                };
                result
                    .symbols
                    .push(symbol(source, node, file_path, "rust", name.clone(), kind));
                push_call_edges(node, source, &name, &mut result.edges, &[]);
            }
            return;
        }
        "function_signature_item" => {
            if let Some(container) = scope.container {
                if let Some(name_node) = child_by_field(node, "name")
                    .or_else(|| first_named_child_of_kind(node, &["identifier"]))
                {
                    let name = qualified(Some(container), &text(source, name_node));
                    result
                        .symbols
                        .push(symbol(source, node, file_path, "rust", name, "method"));
                }
            }
            return;
        }
        "struct_item" | "trait_item" => {
            handle_named_class(node, source, file_path, result);
            if node.kind() == "trait_item" {
                let Some(name_node) = child_by_field(node, "name").or_else(|| {
                    first_named_child_of_kind(node, &["type_identifier", "identifier"])
                }) else {
                    return;
                };
                let name = text(source, name_node);
                walk_children_with_scope(node, source, file_path, result, Some(&name));
            }
            return;
        }
        "enum_item" => {
            handle_enum(node, source, file_path, result);
            return;
        }
        "impl_item" => {
            handle_impl(node, source, file_path, result);
            return;
        }
        "type_item" | "const_item" | "static_item" => {
            if let Some(name_node) = child_by_field(node, "name")
                .or_else(|| first_named_child_of_kind(node, &["type_identifier", "identifier"]))
            {
                let name = text(source, name_node);
                result
                    .symbols
                    .push(symbol(source, node, file_path, "rust", name, "variable"));
            }
            return;
        }
        "macro_definition" => {
            if let Some(name_node) = first_named_child_of_kind(node, &["identifier"]) {
                let name = text(source, name_node);
                result
                    .symbols
                    .push(symbol(source, node, file_path, "rust", name, "macro"));
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

fn walk_children_with_scope(
    node: Node<'_>,
    source: &[u8],
    file_path: &str,
    result: &mut ParseResult,
    container: Option<&str>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_node(
            child,
            source,
            file_path,
            result,
            Scope {
                container,
                in_function: false,
            },
        );
    }
}

fn handle_named_class(node: Node<'_>, source: &[u8], file_path: &str, result: &mut ParseResult) {
    if let Some(name_node) = child_by_field(node, "name")
        .or_else(|| first_named_child_of_kind(node, &["type_identifier", "identifier"]))
    {
        let name = text(source, name_node);
        result
            .symbols
            .push(symbol(source, node, file_path, "rust", name, "class"));
    }
}

fn handle_enum(node: Node<'_>, source: &[u8], file_path: &str, result: &mut ParseResult) {
    let Some(name_node) = child_by_field(node, "name")
        .or_else(|| first_named_child_of_kind(node, &["type_identifier", "identifier"]))
    else {
        return;
    };
    let name = text(source, name_node);
    result.symbols.push(symbol(
        source,
        node,
        file_path,
        "rust",
        name.clone(),
        "class",
    ));
    walk_preorder(node, &mut |variant| {
        if variant.kind() != "enum_variant" {
            return;
        }
        if let Some(variant_name) = first_named_child_of_kind(variant, &["identifier"]) {
            result.symbols.push(symbol(
                source,
                variant,
                file_path,
                "rust",
                qualified(Some(&name), &text(source, variant_name)),
                "variable",
            ));
        }
    });
}

fn handle_impl(node: Node<'_>, source: &[u8], file_path: &str, result: &mut ParseResult) {
    let impl_text = text(source, node);
    let (trait_name, type_name) = impl_header_names(&impl_text);
    if let (Some(trait_name), Some(implementor)) = (trait_name.as_deref(), type_name.as_deref()) {
        result
            .edges
            .push(edge(implementor, trait_name, "implements", None));
        result
            .edges
            .push(edge(trait_name, implementor, "implemented_by", None));
        walk_children_with_scope(node, source, file_path, result, Some(implementor));
        return;
    }
    if impl_header_has_trait_for(&impl_text) {
        let mut types = Vec::new();
        walk_preorder(node, &mut |candidate| {
            if candidate.kind() == "type_identifier" {
                types.push(clean_impl_type_name(&text(source, candidate)));
            }
        });
        if types.len() >= 2 {
            let trait_name = &types[0];
            let implementor = &types[1];
            result
                .edges
                .push(edge(implementor, trait_name, "implements", None));
            result
                .edges
                .push(edge(trait_name, implementor, "implemented_by", None));
            walk_children_with_scope(node, source, file_path, result, Some(implementor));
            return;
        }
    }
    if let Some(type_name) = type_name {
        walk_children_with_scope(node, source, file_path, result, Some(&type_name));
    }
}

fn impl_header_names(impl_text: &str) -> (Option<String>, Option<String>) {
    let Some(header) = impl_text.split('{').next() else {
        return (None, None);
    };
    let rest = header.trim().strip_prefix("impl").unwrap_or(header).trim();
    let rest = strip_leading_impl_generics(rest);
    if let Some((trait_name, implementor)) = rest.split_once(" for ") {
        return (
            nonempty_clean_impl_type_name(trait_name),
            nonempty_clean_impl_type_name(implementor),
        );
    }
    (None, nonempty_clean_impl_type_name(rest))
}

fn impl_header_has_trait_for(impl_text: &str) -> bool {
    let Some(header) = impl_text.split('{').next() else {
        return false;
    };
    let rest = header.trim().strip_prefix("impl").unwrap_or(header).trim();
    strip_leading_impl_generics(rest).contains(" for ")
}

fn strip_leading_impl_generics(value: &str) -> &str {
    let value = value.trim_start();
    if !value.starts_with('<') {
        return value;
    }
    let mut depth = 0_i32;
    for (index, character) in value.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return value[index + character.len_utf8()..].trim_start();
                }
            }
            _ => {}
        }
    }
    value
}

fn nonempty_clean_impl_type_name(value: &str) -> Option<String> {
    let cleaned = clean_impl_type_name(value);
    (!cleaned.is_empty()).then_some(cleaned)
}

fn clean_impl_type_name(value: &str) -> String {
    let mut cleaned = value.trim();
    while let Some(stripped) = cleaned.strip_prefix('&') {
        cleaned = stripped.trim_start();
    }
    cleaned
        .split(" where ")
        .next()
        .unwrap_or(cleaned)
        .split_whitespace()
        .next()
        .unwrap_or(cleaned)
        .split('<')
        .next()
        .unwrap_or(cleaned)
        .rsplit("::")
        .next()
        .unwrap_or(cleaned)
        .trim()
        .to_string()
}

fn handle_use(node: Node<'_>, source: &[u8], result: &mut ParseResult) {
    let Some(argument) = child_by_field(node, "argument") else {
        return;
    };
    handle_use_item(argument, source, "", &mut result.edges);
}

fn handle_use_item(
    node: Node<'_>,
    source: &[u8],
    prefix: &str,
    edges: &mut Vec<crate::models::ParsedEdge>,
) {
    match node.kind() {
        "use_wildcard" => {}
        "scoped_use_list" => {
            let next_prefix = child_by_field(node, "path")
                .map(|path| join_rust_path(prefix, &text(source, path)))
                .unwrap_or_else(|| prefix.to_string());
            if let Some(list) = child_by_field(node, "list") {
                handle_use_item(list, source, &next_prefix, edges);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                handle_use_item(child, source, prefix, edges);
            }
        }
        "use_as_clause" => {
            let Some(path_node) = child_by_field(node, "path") else {
                return;
            };
            let Some(alias_node) = child_by_field(node, "alias") else {
                return;
            };
            let path = text(source, path_node);
            let full_path = join_rust_path(prefix, &path);
            let target = rust_path_leaf(&path).to_string();
            let local = text(source, alias_node);
            edges.push(edge(local, target, "imports", Some(full_path)));
        }
        "scoped_identifier" | "identifier" | "crate" | "self" | "super" => {
            let path = text(source, node);
            let full_path = join_rust_path(prefix, &path);
            let name = rust_path_leaf(&path).to_string();
            edges.push(edge(name.clone(), name, "imports", Some(full_path)));
        }
        _ => {}
    }
}

fn join_rust_path(prefix: &str, path: &str) -> String {
    if prefix.is_empty() {
        path.to_string()
    } else if path.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}::{path}")
    }
}

fn rust_path_leaf(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}
