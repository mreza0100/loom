"""Go language adapter for Loom."""

import logging
from pathlib import Path

import tree_sitter_go as tsgo
from tree_sitter import Language, Node, Parser

from loom.store.models import ParsedEdge, Symbol

log = logging.getLogger(__name__)

GO_LANGUAGE = Language(tsgo.language())

_GO_EXTENSIONS: frozenset[str] = frozenset({".go"})

_GO_EXCLUDED_DIRS: frozenset[str] = frozenset({"vendor"})


class GoAdapter:
    """LanguageAdapter for Go source files."""

    extensions: frozenset[str] = _GO_EXTENSIONS
    language_name: str = "go"
    excluded_dirs: frozenset[str] = _GO_EXCLUDED_DIRS

    def parse(
        self,
        source: bytes,
        file_path: str,
    ) -> tuple[list[Symbol], list[ParsedEdge]]:
        """Parse Go source bytes and extract symbols and edges."""
        suffix = Path(file_path).suffix
        if suffix not in _GO_EXTENSIONS:
            return [], []

        parser = Parser(GO_LANGUAGE)
        tree = parser.parse(source)
        root = tree.root_node

        symbols: list[Symbol] = []
        edges: list[ParsedEdge] = []

        _walk_node(root, source, file_path, symbols, edges)

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
        """Resolve a Go package import path to an actual indexed file.

        Go imports are package paths (e.g., github.com/example/pkg/util).
        Matches by progressively longer tail suffixes against known file paths.
        Returns import_path unchanged if no match found.
        """
        if import_path in known_files:
            return import_path

        # Try matching the final path segment(s) as a directory prefix
        parts = import_path.strip("/").split("/")
        for start in range(len(parts) - 1, -1, -1):
            tail = "/".join(parts[start:])
            for f in known_files:
                if f.startswith(tail + "/") or f == tail:
                    return f

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
) -> None:
    ntype = node.type

    if ntype == "import_declaration":
        _handle_import_declaration(node, source, file_path, edges)
        return

    if ntype == "function_declaration":
        _handle_function_declaration(node, source, file_path, symbols, edges)
        return

    if ntype == "method_declaration":
        _handle_method_declaration(node, source, file_path, symbols, edges)
        return

    if ntype == "type_declaration":
        _handle_type_declaration(node, source, file_path, symbols, edges)
        return

    if ntype == "const_declaration":
        _handle_const_var_declaration(node, source, file_path, symbols, "const_spec")
        return

    if ntype == "var_declaration":
        _handle_const_var_declaration(node, source, file_path, symbols, "var_spec")
        return

    for child in node.children:
        _walk_node(child, source, file_path, symbols, edges)


def _handle_import_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    """Handle single and grouped import declarations."""

    def _process_import_spec(spec: Node) -> None:
        # import_spec has an interpreted_string_literal child
        for child in spec.children:
            if child.type == "interpreted_string_literal":
                # The content is inside — look for interpreted_string_literal_content
                content_node = _child_by_type(child, "interpreted_string_literal_content")
                if content_node:
                    path = _get_text(content_node, source)
                else:
                    # Fallback: strip quotes from the literal itself
                    raw = _get_text(child, source)
                    path = raw.strip('"')
                if path:
                    edges.append(
                        ParsedEdge(
                            source_name=path,
                            target_name=path,
                            target_file=path,
                            relationship="imports",
                        )
                    )

    for child in node.children:
        if child.type == "import_spec":
            _process_import_spec(child)
        elif child.type == "import_spec_list":
            for sub in child.children:
                if sub.type == "import_spec":
                    _process_import_spec(sub)


def _handle_function_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    name = _get_text(name_node, source)
    symbols.append(
        Symbol(
            name=name,
            kind="function",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="go",
            context=_get_context(source, node),
        )
    )
    body = _child_by_type(node, "block")
    if body:
        _extract_calls(body, source, name, file_path, edges)


def _handle_method_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
) -> None:
    """Extract method with receiver: `func (s S) MethodName() {}`."""
    receiver_type: str | None = None
    method_name: str | None = None

    for child in node.children:
        if child.type == "parameter_list" and receiver_type is None:
            # First parameter_list is the receiver
            for sub in child.children:
                if sub.type == "parameter_declaration":
                    # Type is either type_identifier or pointer_type → type_identifier
                    type_node = _child_by_type(sub, "type_identifier")
                    if type_node is None:
                        ptr = _child_by_type(sub, "pointer_type")
                        if ptr:
                            type_node = _child_by_type(ptr, "type_identifier")
                    if type_node:
                        receiver_type = _get_text(type_node, source)
        elif child.type == "field_identifier":
            method_name = _get_text(child, source)

    if receiver_type and method_name:
        full_name = f"{receiver_type}.{method_name}"
        symbols.append(
            Symbol(
                name=full_name,
                kind="method",
                file=file_path,
                line=node.start_point[0] + 1,
                end_line=node.end_point[0] + 1,
                language="go",
                context=_get_context(source, node),
            )
        )
        body = _child_by_type(node, "block")
        if body:
            _extract_calls(body, source, full_name, file_path, edges)


def _handle_type_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
) -> None:
    """Handle type declarations: struct, interface, and type alias."""
    for child in node.children:
        if child.type == "type_spec":
            _process_type_spec(child, source, file_path, symbols, edges)
        elif child.type == "type_alias":
            # type A = B — kind="variable"
            name_node = _child_by_type(child, "type_identifier")
            if name_node:
                symbols.append(
                    Symbol(
                        name=_get_text(name_node, source),
                        kind="variable",
                        file=file_path,
                        line=child.start_point[0] + 1,
                        end_line=child.end_point[0] + 1,
                        language="go",
                        context=_get_context(source, child),
                    )
                )


def _process_type_spec(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
) -> None:
    name_node = _child_by_type(node, "type_identifier")
    if name_node is None:
        return
    type_name = _get_text(name_node, source)

    kind = "variable"
    struct_node = _child_by_type(node, "struct_type")
    iface_node = _child_by_type(node, "interface_type")

    if struct_node:
        kind = "class"
        symbols.append(
            Symbol(
                name=type_name,
                kind=kind,
                file=file_path,
                line=node.start_point[0] + 1,
                end_line=node.end_point[0] + 1,
                language="go",
                context=_get_context(source, node),
            )
        )
        # Extract struct embedding
        field_list = _child_by_type(struct_node, "field_declaration_list")
        if field_list:
            for field in field_list.children:
                if field.type == "field_declaration":
                    _check_embedding(field, source, type_name, file_path, edges)
    elif iface_node:
        kind = "class"
        symbols.append(
            Symbol(
                name=type_name,
                kind=kind,
                file=file_path,
                line=node.start_point[0] + 1,
                end_line=node.end_point[0] + 1,
                language="go",
                context=_get_context(source, node),
            )
        )
    else:
        # Plain type definition (e.g., type MyInt int)
        symbols.append(
            Symbol(
                name=type_name,
                kind="variable",
                file=file_path,
                line=node.start_point[0] + 1,
                end_line=node.end_point[0] + 1,
                language="go",
                context=_get_context(source, node),
            )
        )


def _check_embedding(
    field: Node,
    source: bytes,
    struct_name: str,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    """Check if a field_declaration is an embedded type (no field_identifier)."""
    has_field_identifier = any(c.type == "field_identifier" for c in field.children)
    if has_field_identifier:
        return
    # Embedded field: only a type_identifier or pointer_type child
    type_node = _child_by_type(field, "type_identifier")
    if type_node is None:
        ptr = _child_by_type(field, "pointer_type")
        if ptr:
            type_node = _child_by_type(ptr, "type_identifier")
    if type_node:
        embedded_name = _get_text(type_node, source)
        edges.append(
            ParsedEdge(
                source_name=struct_name,
                target_name=embedded_name,
                target_file=None,
                relationship="extends",
            )
        )
        edges.append(
            ParsedEdge(
                source_name=embedded_name,
                target_name=struct_name,
                target_file=None,
                relationship="extended_by",
            )
        )


def _handle_const_var_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    spec_type: str,
) -> None:
    """Handle const/var declarations at package level."""
    for child in node.children:
        if child.type == spec_type:
            # Each spec can have multiple identifier children
            for sub in child.children:
                if sub.type == "identifier":
                    symbols.append(
                        Symbol(
                            name=_get_text(sub, source),
                            kind="variable",
                            file=file_path,
                            line=sub.start_point[0] + 1,
                            end_line=sub.end_point[0] + 1,
                            language="go",
                            context=_get_context(source, child),
                        )
                    )
        elif child.type == "identifier":
            # Single const without parentheses
            symbols.append(
                Symbol(
                    name=_get_text(child, source),
                    kind="variable",
                    file=file_path,
                    line=child.start_point[0] + 1,
                    end_line=child.end_point[0] + 1,
                    language="go",
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
    """Recursively extract call edges from a Go function/method body."""
    if node.type == "call_expression":
        func_node = node.children[0] if node.children else None
        if func_node and func_node.type in ("identifier", "selector_expression"):
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

    for child in node.children:
        _extract_calls(child, source, caller_name, file_path, edges)
