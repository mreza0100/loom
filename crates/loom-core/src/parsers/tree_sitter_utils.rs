use std::{
    borrow::Cow,
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use tree_sitter::{Node, Parser};

use crate::{
    error::{LoomError, Result},
    models::{ParsedEdge, Symbol},
    parsers::ParseResult,
};

#[derive(Debug, Clone, Copy)]
pub struct Scope<'scope> {
    pub container: Option<&'scope str>,
    pub in_function: bool,
}

impl<'scope> Scope<'scope> {
    pub const TOP: Self = Self {
        container: None,
        in_function: false,
    };
}

pub fn parse_with_language(
    source: &[u8],
    file_path: &str,
    language_name: &str,
    language: tree_sitter::Language,
) -> Result<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|source| LoomError::ParserLanguage {
            language: language_name.to_string(),
            source,
        })?;
    parser
        .parse(source, None)
        .ok_or_else(|| LoomError::ParserNoTree {
            language: language_name.to_string(),
            path: file_path.to_string(),
        })
}

#[must_use]
pub fn node_text<'src>(source: &'src [u8], node: Node<'_>) -> Cow<'src, str> {
    String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()])
}

#[must_use]
pub fn text(source: &[u8], node: Node<'_>) -> String {
    node_text(source, node).into_owned()
}

#[must_use]
pub fn node_context(source: &[u8], node: Node<'_>, max_lines: usize) -> String {
    let start = node.start_position().row;
    let end = (node.end_position().row + 1).min(start + max_lines);
    source
        .split(|byte| *byte == b'\n')
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|line| String::from_utf8_lossy(line))
        .collect::<Vec<Cow<'_, str>>>()
        .join("\n")
}

#[must_use]
pub fn line_start(node: Node<'_>) -> i64 {
    i64::try_from(node.start_position().row + 1).unwrap_or(i64::MAX)
}

#[must_use]
pub fn line_end(node: Node<'_>) -> i64 {
    i64::try_from(node.end_position().row + 1).unwrap_or(i64::MAX)
}

#[must_use]
pub fn child_by_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .find(|child| child.kind() == kind);
    found
}

#[must_use]
pub fn child_by_field<'tree>(node: Node<'tree>, field: &str) -> Option<Node<'tree>> {
    node.child_by_field_name(field)
}

pub fn children_by_kind<'tree>(node: Node<'tree>, kind: &str) -> impl Iterator<Item = Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(move |child| child.kind() == kind)
        .collect::<Vec<_>>()
        .into_iter()
}

#[must_use]
pub fn first_named_child_of_kind<'tree>(node: Node<'tree>, kinds: &[&str]) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .find(|child| child.is_named() && kinds.contains(&child.kind()));
    found
}

#[must_use]
pub fn descendant_of_kind<'tree>(node: Node<'tree>, kinds: &[&str]) -> Option<Node<'tree>> {
    if kinds.contains(&node.kind()) {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = descendant_of_kind(child, kinds) {
            return Some(found);
        }
    }
    None
}

#[must_use]
pub fn named_text(source: &[u8], node: Node<'_>) -> Option<String> {
    first_named_child_of_kind(
        node,
        &[
            "identifier",
            "property_identifier",
            "field_identifier",
            "type_identifier",
            "constant",
            "namespace_identifier",
        ],
    )
    .map(|name| text(source, name))
}

#[must_use]
pub fn strip_string_literal_quotes(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .to_string()
}

#[must_use]
pub fn qualified(container: Option<&str>, name: &str) -> String {
    match container {
        Some(container) if !container.is_empty() => format!("{container}.{name}"),
        _ => name.to_string(),
    }
}

#[must_use]
pub fn symbol(
    source: &[u8],
    node: Node<'_>,
    file_path: &str,
    language: &str,
    name: String,
    kind: &str,
) -> Symbol {
    Symbol {
        id: None,
        name,
        kind: kind.to_string(),
        file: file_path.to_string(),
        line: line_start(node),
        end_line: line_end(node),
        language: language.to_string(),
        context: node_context(source, node, 10),
    }
}

#[must_use]
pub fn edge(
    source_name: impl Into<String>,
    target_name: impl Into<String>,
    relationship: &str,
    target_file: Option<String>,
) -> ParsedEdge {
    ParsedEdge {
        source_name: source_name.into(),
        target_name: target_name.into(),
        relationship: relationship.to_string(),
        target_file,
    }
}

pub fn walk_children<F>(node: Node<'_>, mut visit: F)
where
    F: FnMut(Node<'_>),
{
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child);
    }
}

pub fn walk_preorder<F>(node: Node<'_>, visit: &mut F)
where
    F: FnMut(Node<'_>),
{
    visit(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_preorder(child, visit);
    }
}

#[must_use]
pub fn is_call_kind(kind: &str) -> bool {
    matches!(
        kind,
        "call_expression" | "method_invocation" | "invocation_expression" | "macro_invocation"
    )
}

#[must_use]
pub fn is_comment_only_or_empty(source: &[u8]) -> bool {
    String::from_utf8_lossy(source)
        .lines()
        .map(str::trim)
        .all(|line| line.is_empty() || line.starts_with("//") || line.starts_with('#'))
}

pub fn push_call_edges(
    node: Node<'_>,
    source: &[u8],
    source_name: &str,
    edges: &mut Vec<ParsedEdge>,
    skipped_prefixes: &[&str],
) {
    walk_preorder(node, &mut |candidate| {
        let Some(target) = call_target(candidate, source) else {
            return;
        };
        if target == source_name
            || skipped_prefixes
                .iter()
                .any(|prefix| target.starts_with(prefix))
        {
            return;
        }
        edges.push(edge(source_name, target, "calls", None));
    });
}

#[must_use]
pub fn call_target(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "call_expression" => {
            if let Some(function) = child_by_field(node, "function") {
                return Some(text(source, function));
            }
            first_named_child_of_kind(
                node,
                &[
                    "identifier",
                    "member_expression",
                    "selector_expression",
                    "field_expression",
                    "scoped_identifier",
                    "attribute",
                    "call_expression",
                ],
            )
            .map(|target| text(source, target))
        }
        "call" | "method_invocation" | "invocation_expression" => {
            let raw = text(source, node);
            raw.split('(').next().map(str::trim).map(str::to_string)
        }
        "macro_invocation" => first_named_child_of_kind(node, &["identifier", "scoped_identifier"])
            .map(|target| text(source, target).trim_end_matches('!').to_string()),
        _ => None,
    }
}

pub fn push_instantiates(
    node: Node<'_>,
    source: &[u8],
    source_name: &str,
    edges: &mut Vec<ParsedEdge>,
) {
    walk_preorder(node, &mut |candidate| match candidate.kind() {
        "new_expression"
        | "object_creation_expression"
        | "object_creation_expression_without_initializer" => {
            if let Some(target) = first_named_child_of_kind(
                candidate,
                &[
                    "identifier",
                    "type_identifier",
                    "generic_name",
                    "qualified_name",
                    "scoped_type_identifier",
                ],
            ) {
                edges.push(edge(
                    source_name,
                    text(source, target),
                    "instantiates",
                    None,
                ));
            }
        }
        "call" | "call_expression" => {
            if let Some(target) = first_named_child_of_kind(candidate, &["identifier"]) {
                let name = text(source, target);
                if name.chars().next().is_some_and(char::is_uppercase) {
                    edges.push(edge(source_name, name, "instantiates", None));
                }
            }
        }
        _ => {}
    });
}

#[must_use]
pub fn has_child_kind(node: Node<'_>, kind: &str) -> bool {
    child_by_kind(node, kind).is_some()
}

#[must_use]
pub fn path_parent(path: &str) -> PathBuf {
    Path::new(path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

#[must_use]
pub fn resolve_direct_or_candidates(
    import_path: &str,
    known_files: &BTreeSet<String>,
    candidates: impl IntoIterator<Item = String>,
) -> String {
    if known_files.contains(import_path) {
        return import_path.to_string();
    }
    for candidate in candidates {
        if known_files.contains(&candidate) {
            return candidate;
        }
    }
    import_path.to_string()
}

pub fn language_parse_result(
    source: &[u8],
    file_path: &str,
    language_name: &str,
    language: tree_sitter::Language,
    walk: impl FnOnce(Node<'_>, &mut ParseResult),
) -> Result<ParseResult> {
    let tree = parse_with_language(source, file_path, language_name, language)?;
    let mut result = ParseResult::default();
    walk(tree.root_node(), &mut result);
    Ok(result)
}
