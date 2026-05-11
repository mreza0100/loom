use std::{collections::BTreeSet, path::Path};

use tree_sitter::Node;

use crate::{
    models::ParsedEdge,
    parsers::{
        tree_sitter_utils::{
            child_by_field, child_by_kind, children_by_kind, descendant_of_kind, edge,
            first_named_child_of_kind, language_parse_result, path_parent, push_call_edges,
            push_instantiates, qualified, resolve_direct_or_candidates,
            strip_string_literal_quotes, symbol, text, walk_preorder, Scope,
        },
        LanguageAdapter, ParseResult,
    },
    Result,
};

pub struct PythonAdapter;

impl LanguageAdapter for PythonAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &[".py", ".pyi"]
    }

    fn language_name(&self) -> &'static str {
        "python"
    }

    fn excluded_dirs(&self) -> &'static [&'static str] {
        &[".venv", "venv", ".tox", ".mypy_cache", ".pytest_cache"]
    }

    fn parse(&self, source: &[u8], file_path: &str) -> Result<ParseResult> {
        let extension = Path::new(file_path)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();
        if !matches!(extension, "py" | "pyi") {
            return Ok(ParseResult::default());
        }
        language_parse_result(
            source,
            file_path,
            "python",
            tree_sitter_python::LANGUAGE.into(),
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
        if import_path.starts_with('.') {
            let dots = import_path
                .chars()
                .take_while(|character| *character == '.')
                .count();
            let remainder = import_path.trim_start_matches('.').replace('.', "/");
            let mut base = path_parent(source_file);
            for _ in 1..dots {
                base = base.parent().map(Path::to_path_buf).unwrap_or_default();
            }
            let stem = if remainder.is_empty() {
                base.to_string_lossy().into_owned()
            } else {
                base.join(remainder).to_string_lossy().into_owned()
            };
            return resolve_direct_or_candidates(
                import_path,
                known_files,
                [format!("{stem}.py"), format!("{stem}/__init__.py")],
            );
        }
        let slash = import_path.replace('.', "/");
        resolve_direct_or_candidates(
            import_path,
            known_files,
            [format!("{slash}.py"), format!("{slash}/__init__.py")],
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
        "import_statement" => {
            handle_import_statement(node, source, &mut result.edges);
            return;
        }
        "import_from_statement" => {
            handle_import_from_statement(node, source, &mut result.edges);
            return;
        }
        "function_definition" => {
            if let Some(name_node) = child_by_field(node, "name")
                .or_else(|| first_named_child_of_kind(node, &["identifier"]))
            {
                let name = qualified(scope.container, &text(source, name_node));
                let kind = if scope.container.is_some() {
                    "method"
                } else {
                    "function"
                };
                result.symbols.push(symbol(
                    source,
                    node,
                    file_path,
                    "python",
                    name.clone(),
                    kind,
                ));
                push_call_edges(node, source, &name, &mut result.edges, &[]);
                push_instantiates(node, source, &name, &mut result.edges);
            }
            return;
        }
        "class_definition" => {
            if let Some(name_node) = child_by_field(node, "name")
                .or_else(|| first_named_child_of_kind(node, &["identifier"]))
            {
                let name = qualified(scope.container, &text(source, name_node));
                result.symbols.push(symbol(
                    source,
                    node,
                    file_path,
                    "python",
                    name.clone(),
                    "class",
                ));
                handle_bases(node, source, &name, &mut result.edges);
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
            return;
        }
        "decorated_definition" => {}
        "expression_statement" if scope.container.is_none() && !scope.in_function => {
            handle_upper_assignment(node, source, file_path, result);
        }
        _ => {}
    }
    let next = Scope {
        container: scope.container,
        in_function: scope.in_function || node.kind() == "function_definition",
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_node(child, source, file_path, result, next);
    }
}

fn handle_import_statement(node: Node<'_>, source: &[u8], edges: &mut Vec<ParsedEdge>) {
    walk_preorder(node, &mut |candidate| match candidate.kind() {
        "dotted_name" => {
            let name = text(source, candidate);
            edges.push(edge(name.clone(), name.clone(), "imports", Some(name)));
        }
        "aliased_import" => {
            let module = child_by_kind(candidate, "dotted_name")
                .or_else(|| child_by_kind(candidate, "identifier"))
                .map(|name| text(source, name));
            let alias = children_by_kind(candidate, "identifier")
                .last()
                .map(|name| text(source, name));
            if let Some(module) = module {
                edges.push(edge(
                    alias.unwrap_or_else(|| module.clone()),
                    module.clone(),
                    "imports",
                    Some(module),
                ));
            }
        }
        _ => {}
    });
}

fn handle_import_from_statement(node: Node<'_>, source: &[u8], edges: &mut Vec<ParsedEdge>) {
    if descendant_of_kind(node, &["wildcard_import"]).is_some() {
        return;
    }
    let module = child_by_kind(node, "relative_import")
        .or_else(|| child_by_kind(node, "dotted_name"))
        .map(|module| text(source, module))
        .unwrap_or_default();
    for child in children_by_kind(node, "dotted_name")
        .chain(children_by_kind(node, "identifier"))
        .chain(children_by_kind(node, "aliased_import"))
    {
        let raw = text(source, child);
        if raw == module || raw == "from" || raw == "import" {
            continue;
        }
        let (local, target) = if child.kind() == "aliased_import" {
            let target = child_by_kind(child, "dotted_name")
                .or_else(|| child_by_kind(child, "identifier"))
                .map(|target| text(source, target))
                .unwrap_or_else(|| raw.clone());
            let local = children_by_kind(child, "identifier")
                .last()
                .map(|alias| text(source, alias))
                .unwrap_or_else(|| target.clone());
            (local, target)
        } else {
            (raw.clone(), raw)
        };
        edges.push(edge(local, target, "imports", Some(module.clone())));
    }
}

fn handle_upper_assignment(
    node: Node<'_>,
    source: &[u8],
    file_path: &str,
    result: &mut ParseResult,
) {
    if !matches!(node.kind(), "expression_statement" | "assignment") {
        return;
    }
    let Some(identifier) = descendant_of_kind(node, &["identifier"]) else {
        return;
    };
    let name = text(source, identifier);
    if name
        .chars()
        .all(|character| character.is_uppercase() || character == '_')
    {
        result
            .symbols
            .push(symbol(source, node, file_path, "python", name, "variable"));
    }
}

fn handle_bases(node: Node<'_>, source: &[u8], name: &str, edges: &mut Vec<ParsedEdge>) {
    if let Some(args) = descendant_of_kind(node, &["argument_list"]) {
        for base in children_by_kind(args, "identifier").chain(children_by_kind(args, "attribute"))
        {
            let target = strip_string_literal_quotes(&text(source, base));
            edges.push(edge(name, target.clone(), "extends", None));
            edges.push(edge(target, name, "extended_by", None));
        }
    }
}
