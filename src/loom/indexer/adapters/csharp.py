"""C# language adapter for Loom."""

import logging
from pathlib import Path

import tree_sitter_c_sharp as tscs
from tree_sitter import Language, Node, Parser

from loom.store.models import ParsedEdge, Symbol

log = logging.getLogger(__name__)

CS_LANGUAGE = Language(tscs.language())

_CS_EXTENSIONS: frozenset[str] = frozenset({".cs"})

_CS_EXCLUDED_DIRS: frozenset[str] = frozenset({"bin", "obj", ".vs", "packages"})


class CSharpAdapter:
    """LanguageAdapter for C# source files."""

    extensions: frozenset[str] = _CS_EXTENSIONS
    language_name: str = "csharp"
    excluded_dirs: frozenset[str] = _CS_EXCLUDED_DIRS

    def parse(
        self,
        source: bytes,
        file_path: str,
    ) -> tuple[list[Symbol], list[ParsedEdge]]:
        """Parse C# source bytes and extract symbols and edges."""
        suffix = Path(file_path).suffix
        if suffix not in _CS_EXTENSIONS:
            return [], []

        parser = Parser(CS_LANGUAGE)
        tree = parser.parse(source)
        root = tree.root_node

        symbols: list[Symbol] = []
        edges: list[ParsedEdge] = []

        _walk_node(root, source, file_path, symbols, edges, class_stack=[])

        log.debug(
            "Parsed %s: %d symbols, %d edges",
            Path(file_path).name,
            len(symbols),
            len(edges),
        )
        return symbols, edges

    def resolve_module_path(
        self,
        import_path: str,
        source_file: str,
        known_files: set[str],
    ) -> str:
        """Resolve a C# using directive to a file path.

        C# uses namespaces rather than file paths. Falls through to pipeline
        strategy 4-5 (global name match). Returns import_path unchanged.
        """
        if import_path in known_files:
            return import_path
        return import_path


# ── Module-private helpers ───────────────────────────────────────────────────


def _get_text(node: Node, source: bytes) -> str:
    return source[node.start_byte : node.end_byte].decode("utf-8", errors="replace")


def _get_context(source: bytes, node: Node, max_lines: int = 10) -> str:
    start = node.start_point[0]
    end = min(node.end_point[0] + 1, start + max_lines)
    lines = source.split(b"\n")[start:end]
    return b"\n".join(lines).decode("utf-8", errors="replace")


def _child_by_type(node: Node, type_name: str) -> Node | None:
    for child in node.children:
        if child.type == type_name:
            return child
    return None


def _children_by_type(node: Node, type_name: str) -> list[Node]:
    return [c for c in node.children if c.type == type_name]


def _walk_node(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    class_stack: list[str],
) -> None:
    ntype = node.type

    if ntype == "using_directive":
        _handle_using_directive(node, source, file_path, edges)
        return

    if ntype == "namespace_declaration":
        _handle_namespace_declaration(node, source, file_path, symbols, edges, class_stack)
        return

    if ntype == "class_declaration":
        _handle_class_declaration(node, source, file_path, symbols, edges, class_stack)
        return

    if ntype == "struct_declaration":
        _handle_struct_declaration(node, source, file_path, symbols, edges, class_stack)
        return

    if ntype == "interface_declaration":
        _handle_interface_declaration(node, source, file_path, symbols, edges, class_stack)
        return

    if ntype == "enum_declaration":
        _handle_enum_declaration(node, source, file_path, symbols, class_stack)
        return

    if ntype == "record_declaration":
        _handle_record_declaration(node, source, file_path, symbols, class_stack)
        return

    if ntype == "method_declaration":
        _handle_method_declaration(node, source, file_path, symbols, edges, class_stack)
        return

    if ntype == "constructor_declaration":
        _handle_constructor_declaration(node, source, file_path, symbols, edges, class_stack)
        return

    if ntype == "property_declaration":
        _handle_property_declaration(node, source, file_path, symbols, class_stack)
        return

    if ntype == "field_declaration" and class_stack:
        _handle_field_declaration(node, source, file_path, symbols, class_stack)
        return

    for child in node.children:
        _walk_node(child, source, file_path, symbols, edges, class_stack)


def _handle_using_directive(
    node: Node,
    source: bytes,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    """Handle `using Namespace`, `using static Class`, `using Alias = Type`."""
    has_equals = any(c.type == "=" for c in node.children)

    if has_equals:
        # using Alias = Type — use the RHS (after =)
        found_equals = False
        for child in node.children:
            if child.type == "=":
                found_equals = True
                continue
            if found_equals and child.type in ("identifier", "qualified_name"):
                namespace = _get_text(child, source)
                edges.append(
                    ParsedEdge(
                        source_name=namespace,
                        target_name=namespace,
                        target_file=namespace,
                        relationship="imports",
                    )
                )
                break
    else:
        for child in node.children:
            if child.type in ("identifier", "qualified_name"):
                namespace = _get_text(child, source)
                edges.append(
                    ParsedEdge(
                        source_name=namespace,
                        target_name=namespace,
                        target_file=namespace,
                        relationship="imports",
                    )
                )
                break


def _handle_namespace_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    class_stack: list[str],
) -> None:
    """Recurse into namespace body without emitting a namespace symbol."""
    body = _child_by_type(node, "declaration_list")
    if body:
        for child in body.children:
            _walk_node(child, source, file_path, symbols, edges, class_stack)


def _qualified_name(class_stack: list[str], name: str) -> str:
    if class_stack:
        return f"{class_stack[-1]}.{name}"
    return name


def _extract_base_list(
    node: Node,
    source: bytes,
    class_name: str,
    file_path: str,
    edges: list[ParsedEdge],
    is_class: bool = True,
) -> None:
    """Extract base_list entries and emit extends/implements edges.

    C# base_list is flat (class + interfaces mixed). All entries emit as
    "extends" from a class_declaration. Known trade-off per architecture doc.
    """
    base_list = _child_by_type(node, "base_list")
    if not base_list:
        return

    for child in base_list.children:
        if child.type in ("identifier", "qualified_name", "generic_name"):
            base_name = _get_text(child, source)
            # Skip punctuation tokens
            if base_name in (":", ","):
                continue
            edges.append(
                ParsedEdge(
                    source_name=class_name,
                    target_name=base_name,
                    target_file=None,
                    relationship="extends",
                )
            )
            edges.append(
                ParsedEdge(
                    source_name=base_name,
                    target_name=class_name,
                    target_file=None,
                    relationship="extended_by",
                )
            )


def _handle_class_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    class_stack: list[str],
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    class_name = _get_text(name_node, source)
    qualified = _qualified_name(class_stack, class_name)

    symbols.append(
        Symbol(
            name=qualified,
            kind="class",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="csharp",
            context=_get_context(source, node),
        )
    )

    _extract_base_list(node, source, qualified, file_path, edges, is_class=True)

    body = _child_by_type(node, "declaration_list")
    if body:
        new_stack = class_stack + [qualified]
        for child in body.children:
            _walk_node(child, source, file_path, symbols, edges, new_stack)


def _handle_struct_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    class_stack: list[str],
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    struct_name = _get_text(name_node, source)
    qualified = _qualified_name(class_stack, struct_name)

    symbols.append(
        Symbol(
            name=qualified,
            kind="class",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="csharp",
            context=_get_context(source, node),
        )
    )

    _extract_base_list(node, source, qualified, file_path, edges, is_class=False)

    body = _child_by_type(node, "declaration_list")
    if body:
        new_stack = class_stack + [qualified]
        for child in body.children:
            _walk_node(child, source, file_path, symbols, edges, new_stack)


def _handle_interface_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    class_stack: list[str],
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    iface_name = _get_text(name_node, source)
    qualified = _qualified_name(class_stack, iface_name)

    symbols.append(
        Symbol(
            name=qualified,
            kind="class",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="csharp",
            context=_get_context(source, node),
        )
    )

    _extract_base_list(node, source, qualified, file_path, edges, is_class=False)

    body = _child_by_type(node, "declaration_list")
    if body:
        new_stack = class_stack + [qualified]
        for child in body.children:
            _walk_node(child, source, file_path, symbols, edges, new_stack)


def _handle_enum_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    class_stack: list[str],
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    enum_name = _get_text(name_node, source)
    qualified = _qualified_name(class_stack, enum_name)

    symbols.append(
        Symbol(
            name=qualified,
            kind="class",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="csharp",
            context=_get_context(source, node),
        )
    )

    body = _child_by_type(node, "enum_member_declaration_list")
    if body:
        for child in body.children:
            if child.type == "enum_member_declaration":
                id_node = _child_by_type(child, "identifier")
                if id_node:
                    member_name = _get_text(id_node, source)
                    symbols.append(
                        Symbol(
                            name=f"{qualified}.{member_name}",
                            kind="variable",
                            file=file_path,
                            line=child.start_point[0] + 1,
                            end_line=child.end_point[0] + 1,
                            language="csharp",
                            context=_get_context(source, child),
                        )
                    )


def _handle_record_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    class_stack: list[str],
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    record_name = _get_text(name_node, source)
    qualified = _qualified_name(class_stack, record_name)

    symbols.append(
        Symbol(
            name=qualified,
            kind="class",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="csharp",
            context=_get_context(source, node),
        )
    )


def _handle_method_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    class_stack: list[str],
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    method_name = _get_text(name_node, source)
    qualified = _qualified_name(class_stack, method_name)

    symbols.append(
        Symbol(
            name=qualified,
            kind="method",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="csharp",
            context=_get_context(source, node),
        )
    )

    body = _child_by_type(node, "block")
    if body:
        _extract_calls(body, source, qualified, file_path, edges)


def _handle_constructor_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    class_stack: list[str],
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    ctor_name = _get_text(name_node, source)
    qualified = _qualified_name(class_stack, ctor_name)

    symbols.append(
        Symbol(
            name=qualified,
            kind="method",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="csharp",
            context=_get_context(source, node),
        )
    )

    body = _child_by_type(node, "block")
    if body:
        _extract_calls(body, source, qualified, file_path, edges)


def _handle_property_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    class_stack: list[str],
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    prop_name = _get_text(name_node, source)
    qualified = _qualified_name(class_stack, prop_name)

    symbols.append(
        Symbol(
            name=qualified,
            kind="variable",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="csharp",
            context=_get_context(source, node),
        )
    )


def _handle_field_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    class_stack: list[str],
) -> None:
    """Extract field declarations from class bodies."""
    var_decl = _child_by_type(node, "variable_declaration")
    if var_decl is None:
        return
    for child in var_decl.children:
        if child.type == "variable_declarator":
            id_node = _child_by_type(child, "identifier")
            if id_node:
                field_name = _get_text(id_node, source)
                qualified = _qualified_name(class_stack, field_name)
                symbols.append(
                    Symbol(
                        name=qualified,
                        kind="variable",
                        file=file_path,
                        line=node.start_point[0] + 1,
                        end_line=node.end_point[0] + 1,
                        language="csharp",
                        context=_get_context(source, node),
                    )
                )


def _extract_calls(
    node: Node,
    source: bytes,
    caller_name: str,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    """Recursively extract call and instantiation edges from a C# method body."""
    if node.type == "invocation_expression":
        func_node = node.children[0] if node.children else None
        if func_node:
            if func_node.type == "member_access_expression":
                # Object.Method form
                callee = _get_text(func_node, source)
                if callee != caller_name:
                    edges.append(
                        ParsedEdge(
                            source_name=caller_name,
                            target_name=callee,
                            target_file=None,
                            relationship="calls",
                        )
                    )
            elif func_node.type == "identifier":
                callee = _get_text(func_node, source)
                if callee != caller_name:
                    edges.append(
                        ParsedEdge(
                            source_name=caller_name,
                            target_name=callee,
                            target_file=None,
                            relationship="calls",
                        )
                    )

    elif node.type == "object_creation_expression":
        # new ClassName(...)
        for child in node.children:
            if child.type in ("identifier", "qualified_name", "generic_name"):
                class_name = _get_text(child, source)
                edges.append(
                    ParsedEdge(
                        source_name=caller_name,
                        target_name=class_name,
                        target_file=None,
                        relationship="instantiates",
                    )
                )
                break

    for child in node.children:
        _extract_calls(child, source, caller_name, file_path, edges)
