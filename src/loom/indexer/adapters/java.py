"""Java language adapter for Loom."""

import logging
from pathlib import Path

import tree_sitter_java as tsjava
from tree_sitter import Language, Node, Parser

from loom.store.models import ParsedEdge, Symbol

log = logging.getLogger(__name__)

JAVA_LANGUAGE = Language(tsjava.language())

_JAVA_EXTENSIONS: frozenset[str] = frozenset({".java"})

_JAVA_EXCLUDED_DIRS: frozenset[str] = frozenset({"target", "build", ".gradle", ".idea", "out"})


class JavaAdapter:
    """LanguageAdapter for Java source files."""

    extensions: frozenset[str] = _JAVA_EXTENSIONS
    language_name: str = "java"
    excluded_dirs: frozenset[str] = _JAVA_EXCLUDED_DIRS

    def parse(
        self,
        source: bytes,
        file_path: str,
    ) -> tuple[list[Symbol], list[ParsedEdge]]:
        """Parse Java source bytes and extract symbols and edges."""
        suffix = Path(file_path).suffix
        if suffix not in _JAVA_EXTENSIONS:
            return [], []

        parser = Parser(JAVA_LANGUAGE)
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
        """Resolve a Java import path to an actual indexed file.

        Converts com.example.Foo → com/example/Foo.java and checks known_files.
        Falls back to tail-segment matching.
        Returns import_path unchanged if no match found.
        """
        if import_path in known_files:
            return import_path

        # Convert dot-notation to file path
        slash_path = import_path.replace(".", "/") + ".java"
        if slash_path in known_files:
            return slash_path

        # Tail-segment match: check if any file ends with /LastSegment.java
        last_segment = import_path.split(".")[-1] if "." in import_path else import_path
        candidate_suffix = f"/{last_segment}.java"
        for f in known_files:
            if f.endswith(candidate_suffix):
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
    class_stack: list[str],
) -> None:
    ntype = node.type

    if ntype == "import_declaration":
        _handle_import_declaration(node, source, file_path, edges)
        return

    if ntype == "class_declaration":
        _handle_class_declaration(node, source, file_path, symbols, edges, class_stack)
        return

    if ntype == "interface_declaration":
        _handle_interface_declaration(node, source, file_path, symbols, edges, class_stack)
        return

    if ntype == "enum_declaration":
        _handle_enum_declaration(node, source, file_path, symbols, edges, class_stack)
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

    if ntype == "field_declaration" and class_stack:
        _handle_field_declaration(node, source, file_path, symbols, class_stack)
        return

    for child in node.children:
        _walk_node(child, source, file_path, symbols, edges, class_stack)


def _handle_import_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    """Handle Java import declarations. Skip wildcard imports."""
    # Check for wildcard import (asterisk child)
    has_asterisk = any(c.type == "asterisk" for c in node.children)
    if has_asterisk:
        log.warning("Skipping wildcard import in %s", file_path)
        return

    # Extract the scoped_identifier
    scoped = _child_by_type(node, "scoped_identifier")
    if scoped:
        import_text = _get_text(scoped, source)
        edges.append(
            ParsedEdge(
                source_name=import_text,
                target_name=import_text,
                target_file=import_text,
                relationship="imports",
            )
        )


def _qualified_name(class_stack: list[str], name: str) -> str:
    if class_stack:
        return f"{class_stack[-1]}.{name}"
    return name


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
            language="java",
            context=_get_context(source, node),
        )
    )

    # Extract superclass
    superclass = _child_by_type(node, "superclass")
    if superclass:
        type_id = _child_by_type(superclass, "type_identifier")
        if type_id:
            parent = _get_text(type_id, source)
            edges.append(
                ParsedEdge(
                    source_name=qualified,
                    target_name=parent,
                    target_file=None,
                    relationship="extends",
                )
            )
            edges.append(
                ParsedEdge(
                    source_name=parent,
                    target_name=qualified,
                    target_file=None,
                    relationship="extended_by",
                )
            )

    # Extract implemented interfaces
    super_ifaces = _child_by_type(node, "super_interfaces")
    if super_ifaces:
        type_list = _child_by_type(super_ifaces, "type_list")
        if type_list:
            for child in type_list.children:
                if child.type == "type_identifier":
                    iface = _get_text(child, source)
                    edges.append(
                        ParsedEdge(
                            source_name=qualified,
                            target_name=iface,
                            target_file=None,
                            relationship="implements",
                        )
                    )

    # Recurse into class body
    body = _child_by_type(node, "class_body")
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
            language="java",
            context=_get_context(source, node),
        )
    )

    # Extract extended interfaces
    extends_ifaces = _child_by_type(node, "extends_interfaces")
    if extends_ifaces:
        type_list = _child_by_type(extends_ifaces, "type_list")
        if type_list:
            for child in type_list.children:
                if child.type == "type_identifier":
                    parent = _get_text(child, source)
                    edges.append(
                        ParsedEdge(
                            source_name=qualified,
                            target_name=parent,
                            target_file=None,
                            relationship="extends",
                        )
                    )

    # Recurse into interface body
    body = _child_by_type(node, "interface_body")
    if body:
        new_stack = class_stack + [qualified]
        for child in body.children:
            _walk_node(child, source, file_path, symbols, edges, new_stack)


def _handle_enum_declaration(
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
    enum_name = _get_text(name_node, source)
    qualified = _qualified_name(class_stack, enum_name)

    symbols.append(
        Symbol(
            name=qualified,
            kind="class",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="java",
            context=_get_context(source, node),
        )
    )

    # Extract enum constants from enum_body
    body = _child_by_type(node, "enum_body")
    if body:
        for child in body.children:
            if child.type == "enum_constant":
                id_node = _child_by_type(child, "identifier")
                if id_node:
                    const_name = _get_text(id_node, source)
                    symbols.append(
                        Symbol(
                            name=f"{qualified}.{const_name}",
                            kind="variable",
                            file=file_path,
                            line=child.start_point[0] + 1,
                            end_line=child.end_point[0] + 1,
                            language="java",
                            context=_get_context(source, child),
                        )
                    )
            elif child.type == "enum_body_declarations":
                # Methods and fields inside enum
                new_stack = class_stack + [qualified]
                for sub in child.children:
                    _walk_node(sub, source, file_path, symbols, edges, new_stack)


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
            language="java",
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
            language="java",
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
            language="java",
            context=_get_context(source, node),
        )
    )

    body = _child_by_type(node, "constructor_body")
    if body:
        _extract_calls(body, source, qualified, file_path, edges)


def _handle_field_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    class_stack: list[str],
) -> None:
    for child in node.children:
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
                        language="java",
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
    """Recursively extract call and instantiation edges from a Java method body."""
    if node.type == "method_invocation":
        method_node = _child_by_type(node, "identifier")
        if method_node:
            method_name = _get_text(method_node, source)
            # Try to get the object
            obj_node = None
            for child in node.children:
                if child.type in ("identifier", "scoped_identifier") and child is not method_node:
                    obj_node = child
                    break
            callee = f"{_get_text(obj_node, source)}.{method_name}" if obj_node else method_name
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
        type_node = _child_by_type(node, "type_identifier")
        if type_node:
            class_name = _get_text(type_node, source)
            edges.append(
                ParsedEdge(
                    source_name=caller_name,
                    target_name=class_name,
                    target_file=None,
                    relationship="instantiates",
                )
            )

    for child in node.children:
        _extract_calls(child, source, caller_name, file_path, edges)
