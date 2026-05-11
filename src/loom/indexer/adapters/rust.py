"""Rust language adapter for Loom."""

import logging
from pathlib import Path

import tree_sitter_rust as tsrust
from tree_sitter import Language, Node, Parser

from loom.store.models import ParsedEdge, Symbol

log = logging.getLogger(__name__)

RUST_LANGUAGE = Language(tsrust.language())

_RUST_EXTENSIONS: frozenset[str] = frozenset({".rs"})

_RUST_EXCLUDED_DIRS: frozenset[str] = frozenset({"target"})


class RustAdapter:
    """LanguageAdapter for Rust source files."""

    extensions: frozenset[str] = _RUST_EXTENSIONS
    language_name: str = "rust"
    excluded_dirs: frozenset[str] = _RUST_EXCLUDED_DIRS

    def parse(
        self,
        source: bytes,
        file_path: str,
    ) -> tuple[list[Symbol], list[ParsedEdge]]:
        """Parse Rust source bytes and extract symbols and edges."""
        suffix = Path(file_path).suffix
        if suffix not in _RUST_EXTENSIONS:
            return [], []

        parser = Parser(RUST_LANGUAGE)
        tree = parser.parse(source)
        root = tree.root_node

        symbols: list[Symbol] = []
        edges: list[ParsedEdge] = []

        _walk_node(root, source, file_path, symbols, edges, impl_type=None)

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
        """Resolve a Rust use/mod path to an actual indexed file.

        Handles crate::, super::, and bare mod names.
        Returns import_path unchanged if no match found.
        """
        if import_path in known_files:
            return import_path

        source_dir = str(Path(source_file).parent)

        # Strip crate:: prefix → resolve from project root
        if import_path.startswith("crate::"):
            remainder = import_path[len("crate::") :]
            path_part = remainder.replace("::", "/")
            for f in known_files:
                # Match files that end with /path.rs, /path/mod.rs, or equal path.rs
                if f.endswith(f"/{path_part}.rs") or f == path_part + ".rs":
                    return f
                if f.endswith(f"/{path_part}/mod.rs") or f == path_part + "/mod.rs":
                    return f

        # Strip super:: prefix → resolve from parent directory
        elif import_path.startswith("super::"):
            remainder = import_path[len("super::") :]
            path_part = remainder.replace("::", "/")
            parent_dir = str(Path(source_dir).parent)
            for candidate in (
                f"{parent_dir}/{path_part}.rs",
                f"{parent_dir}/{path_part}/mod.rs",
            ):
                if candidate in known_files:
                    return candidate

        else:
            # mod foo style — try source file's directory
            path_part = import_path.replace("::", "/")
            for candidate in (
                f"{source_dir}/{path_part}.rs",
                f"{source_dir}/{path_part}/mod.rs",
                path_part + ".rs",
                path_part + "/mod.rs",
            ):
                if candidate in known_files:
                    return candidate

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
    impl_type: str | None,
) -> None:
    ntype = node.type

    if ntype == "use_declaration":
        _handle_use_declaration(node, source, file_path, edges)
        return

    if ntype == "function_item":
        _handle_function_item(node, source, file_path, symbols, edges, impl_type)
        return

    if ntype == "function_signature_item":
        # Trait method stub — extract as method if in impl_type context
        if impl_type:
            _handle_function_signature(node, source, file_path, symbols, impl_type)
        return

    if ntype == "struct_item":
        _handle_struct_item(node, source, file_path, symbols)
        return

    if ntype == "enum_item":
        _handle_enum_item(node, source, file_path, symbols)
        return

    if ntype == "trait_item":
        _handle_trait_item(node, source, file_path, symbols, edges)
        return

    if ntype == "impl_item":
        _handle_impl_item(node, source, file_path, symbols, edges)
        return

    if ntype == "type_item":
        _handle_type_item(node, source, file_path, symbols)
        return

    if ntype == "const_item":
        _handle_const_static_item(node, source, file_path, symbols, "identifier")
        return

    if ntype == "static_item":
        _handle_const_static_item(node, source, file_path, symbols, "identifier")
        return

    if ntype == "macro_definition":
        _handle_macro_definition(node, source, file_path, symbols)
        return

    for child in node.children:
        _walk_node(child, source, file_path, symbols, edges, impl_type)


def _handle_use_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    """Handle `use` statements. Skip glob imports (`use ...*`)."""
    for child in node.children:
        if child.type == "use_wildcard":
            log.warning("Skipping glob use import in %s", file_path)
            return
        if child.type == "scoped_identifier":
            path_text = _get_text(child, source)
            # Extract last segment as symbol name
            last_part = path_text.split("::")[-1]
            edges.append(
                ParsedEdge(
                    source_name=last_part,
                    target_name=last_part,
                    target_file=path_text,
                    relationship="imports",
                )
            )
        elif child.type == "scoped_use_list":
            # e.g., use foo::{A, B}
            _handle_scoped_use_list(child, source, file_path, edges)
        elif child.type == "use_list":
            _handle_use_list(child, source, file_path, edges)
        elif child.type == "use_as_clause":
            # use X as Y — emit edge for the original path X
            orig_node = child.children[0] if child.children else None
            if orig_node:
                orig = _get_text(orig_node, source)
                last_part = orig.split("::")[-1]
                edges.append(
                    ParsedEdge(
                        source_name=last_part,
                        target_name=last_part,
                        target_file=orig,
                        relationship="imports",
                    )
                )
        elif child.type == "identifier":
            name = _get_text(child, source)
            edges.append(
                ParsedEdge(
                    source_name=name,
                    target_name=name,
                    target_file=name,
                    relationship="imports",
                )
            )


def _handle_scoped_use_list(
    node: Node,
    source: bytes,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    """Handle `use foo::{A, B, C}`."""
    # Find the prefix (scoped_identifier before the use_list)
    prefix = ""
    for child in node.children:
        if child.type in ("scoped_identifier", "identifier"):
            prefix = _get_text(child, source)
        elif child.type == "use_list":
            _handle_use_list(child, source, file_path, edges, prefix=prefix)


def _handle_use_list(
    node: Node,
    source: bytes,
    file_path: str,
    edges: list[ParsedEdge],
    prefix: str = "",
) -> None:
    """Handle `{A, B, C}` in a use statement."""
    for child in node.children:
        if child.type == "use_wildcard":
            log.warning("Skipping glob use import in %s", file_path)
        elif child.type == "identifier":
            name = _get_text(child, source)
            full_path = f"{prefix}::{name}" if prefix else name
            edges.append(
                ParsedEdge(
                    source_name=name,
                    target_name=name,
                    target_file=full_path,
                    relationship="imports",
                )
            )
        elif child.type == "scoped_identifier":
            path_text = _get_text(child, source)
            last_part = path_text.split("::")[-1]
            full_path = f"{prefix}::{path_text}" if prefix else path_text
            edges.append(
                ParsedEdge(
                    source_name=last_part,
                    target_name=last_part,
                    target_file=full_path,
                    relationship="imports",
                )
            )
        elif child.type == "use_as_clause":
            # use X as Y
            orig_node = child.children[0] if child.children else None
            if orig_node:
                orig = _get_text(orig_node, source)
                last_part = orig.split("::")[-1]
                full_path = f"{prefix}::{orig}" if prefix else orig
                edges.append(
                    ParsedEdge(
                        source_name=last_part,
                        target_name=last_part,
                        target_file=full_path,
                        relationship="imports",
                    )
                )


def _handle_function_item(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    impl_type: str | None,
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    raw_name = _get_text(name_node, source)

    if impl_type:
        full_name = f"{impl_type}.{raw_name}"
        kind = "method"
    else:
        full_name = raw_name
        kind = "function"

    symbols.append(
        Symbol(
            name=full_name,
            kind=kind,
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="rust",
            context=_get_context(source, node),
        )
    )

    body = _child_by_type(node, "block")
    if body:
        _extract_calls(body, source, full_name, file_path, edges)


def _handle_function_signature(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    impl_type: str,
) -> None:
    """Extract trait method stub as a method symbol."""
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    raw_name = _get_text(name_node, source)
    full_name = f"{impl_type}.{raw_name}"
    symbols.append(
        Symbol(
            name=full_name,
            kind="method",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="rust",
            context=_get_context(source, node),
        )
    )


def _handle_struct_item(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
) -> None:
    name_node = _child_by_type(node, "type_identifier")
    if name_node is None:
        return
    symbols.append(
        Symbol(
            name=_get_text(name_node, source),
            kind="class",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="rust",
            context=_get_context(source, node),
        )
    )


def _handle_enum_item(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
) -> None:
    name_node = _child_by_type(node, "type_identifier")
    if name_node is None:
        return
    enum_name = _get_text(name_node, source)

    symbols.append(
        Symbol(
            name=enum_name,
            kind="class",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="rust",
            context=_get_context(source, node),
        )
    )

    # Extract enum variants
    variant_list = _child_by_type(node, "enum_variant_list")
    if variant_list:
        for child in variant_list.children:
            if child.type == "enum_variant":
                id_node = _child_by_type(child, "identifier")
                if id_node:
                    variant_name = _get_text(id_node, source)
                    symbols.append(
                        Symbol(
                            name=f"{enum_name}.{variant_name}",
                            kind="variable",
                            file=file_path,
                            line=child.start_point[0] + 1,
                            end_line=child.end_point[0] + 1,
                            language="rust",
                            context=_get_context(source, child),
                        )
                    )


def _handle_trait_item(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
) -> None:
    name_node = _child_by_type(node, "type_identifier")
    if name_node is None:
        return
    trait_name = _get_text(name_node, source)

    symbols.append(
        Symbol(
            name=trait_name,
            kind="class",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="rust",
            context=_get_context(source, node),
        )
    )

    # Extract method stubs from the trait body
    body = _child_by_type(node, "declaration_list")
    if body:
        for child in body.children:
            if child.type == "function_signature_item":
                _handle_function_signature(child, source, file_path, symbols, trait_name)
            elif child.type == "function_item":
                _handle_function_item(child, source, file_path, symbols, edges, trait_name)


def _handle_impl_item(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
) -> None:
    """Handle impl blocks: `impl StructName {}` and `impl Trait for Struct {}`."""
    # Collect type_identifier children and check for 'for' keyword
    type_ids: list[Node] = []
    has_for = False

    for child in node.children:
        if child.type == "type_identifier":
            type_ids.append(child)
        elif child.type == "for":
            has_for = True
        elif child.type in ("generic_type",):
            # e.g., impl<T> Trait<T> for Struct<T>
            inner = _child_by_type(child, "type_identifier")
            if inner:
                type_ids.append(inner)

    if not type_ids:
        return

    if has_for and len(type_ids) >= 2:
        # impl TraitName for StructName
        trait_name = _get_text(type_ids[0], source)
        struct_name = _get_text(type_ids[-1], source)
        # Emit implements edges
        edges.append(
            ParsedEdge(
                source_name=struct_name,
                target_name=trait_name,
                target_file=None,
                relationship="implements",
            )
        )
        edges.append(
            ParsedEdge(
                source_name=trait_name,
                target_name=struct_name,
                target_file=None,
                relationship="implemented_by",
            )
        )
        impl_type = struct_name
    else:
        struct_name = _get_text(type_ids[-1], source)
        impl_type = struct_name

    # Recurse into the declaration_list with impl_type set
    body = _child_by_type(node, "declaration_list")
    if body:
        for child in body.children:
            _walk_node(child, source, file_path, symbols, edges, impl_type)


def _handle_type_item(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
) -> None:
    name_node = _child_by_type(node, "type_identifier")
    if name_node is None:
        return
    symbols.append(
        Symbol(
            name=_get_text(name_node, source),
            kind="variable",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="rust",
            context=_get_context(source, node),
        )
    )


def _handle_const_static_item(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    name_type: str,
) -> None:
    name_node = _child_by_type(node, name_type)
    if name_node is None:
        return
    symbols.append(
        Symbol(
            name=_get_text(name_node, source),
            kind="variable",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="rust",
            context=_get_context(source, node),
        )
    )


def _handle_macro_definition(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
) -> None:
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    symbols.append(
        Symbol(
            name=_get_text(name_node, source),
            kind="macro",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="rust",
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
    """Recursively extract call edges from a Rust function/method body."""
    if node.type == "call_expression":
        func_node = node.children[0] if node.children else None
        if func_node and func_node.type in ("identifier", "scoped_identifier", "field_expression"):
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
