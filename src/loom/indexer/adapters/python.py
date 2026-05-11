"""Python language adapter for Loom."""

import logging
from pathlib import Path

import tree_sitter_python as tspy
from tree_sitter import Language, Node, Parser

from loom.store.models import ParsedEdge, Symbol

log = logging.getLogger(__name__)

PY_LANGUAGE = Language(tspy.language())

_PY_EXTENSIONS: frozenset[str] = frozenset({".py", ".pyi"})

_PY_EXCLUDED_DIRS: frozenset[str] = frozenset(
    {".venv", "venv", ".tox", ".mypy_cache", ".pytest_cache"}
)


class PythonAdapter:
    """LanguageAdapter for Python source files."""

    extensions: frozenset[str] = _PY_EXTENSIONS
    language_name: str = "python"
    excluded_dirs: frozenset[str] = _PY_EXCLUDED_DIRS

    def parse(
        self,
        source: bytes,
        file_path: str,
    ) -> tuple[list[Symbol], list[ParsedEdge]]:
        """Parse Python source bytes and extract symbols and edges."""
        suffix = Path(file_path).suffix
        if suffix not in _PY_EXTENSIONS:
            return [], []

        parser = Parser(PY_LANGUAGE)
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
        """Resolve a Python module import path to an actual indexed file.

        Tries direct match, dot-to-slash conversion, and relative imports.
        Returns import_path unchanged if no match found.
        """
        # 1. Direct match
        if import_path in known_files:
            return import_path

        # 2. Relative import — starts with one or more dots
        if import_path.startswith("."):
            dots = len(import_path) - len(import_path.lstrip("."))
            remainder = import_path.lstrip(".")
            # Walk up 'dots' levels from source_file's directory
            base = Path(source_file).parent
            for _ in range(dots - 1):
                base = base.parent
            rel_path = str(base / remainder.replace(".", "/")) if remainder else str(base)
            for candidate in (rel_path + ".py", rel_path + "/__init__.py"):
                if candidate in known_files:
                    return candidate
            return import_path

        # 3. Convert dots to slashes for absolute dotted imports (foo.bar → foo/bar)
        slash_path = import_path.replace(".", "/")
        for candidate in (slash_path + ".py", slash_path + "/__init__.py"):
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
    class_stack: list[str],
) -> None:
    ntype = node.type

    if ntype == "import_statement":
        _handle_import_statement(node, source, file_path, edges)
        return

    if ntype == "import_from_statement":
        _handle_import_from_statement(node, source, file_path, edges)
        return

    if ntype == "decorated_definition":
        _handle_decorated_definition(node, source, file_path, symbols, edges, class_stack)
        return

    if ntype == "function_definition":
        _handle_function_definition(
            node, source, file_path, symbols, edges, class_stack, context=""
        )
        return

    if ntype == "class_definition":
        _handle_class_definition(node, source, file_path, symbols, edges, class_stack, context="")
        return

    if ntype == "expression_statement" and not class_stack:
        _handle_expression_statement(node, source, file_path, symbols)
        return

    for child in node.children:
        _walk_node(child, source, file_path, symbols, edges, class_stack)


def _handle_import_statement(
    node: Node,
    source: bytes,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    """Handle `import X` and `import X.Y`."""
    for child in node.children:
        if child.type in ("dotted_name", "aliased_import"):
            # For aliased_import, get the first dotted_name child
            if child.type == "aliased_import":
                name_node = _child_by_type(child, "dotted_name")
                if name_node is None:
                    continue
                module_name = _get_text(name_node, source)
            else:
                module_name = _get_text(child, source)
            edges.append(
                ParsedEdge(
                    source_name=module_name,
                    target_name=module_name,
                    target_file=module_name,
                    relationship="imports",
                )
            )


def _handle_import_from_statement(
    node: Node,
    source: bytes,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    """Handle `from X import Y` and `from . import Y`."""
    module_name: str | None = None
    imported_names: list[str] = []

    for child in node.children:
        if child.type == "dotted_name":
            if module_name is None:
                module_name = _get_text(child, source)
            else:
                imported_names.append(_get_text(child, source))
        elif child.type == "relative_import":
            module_name = _get_text(child, source)
        elif child.type == "import_from_as_names":
            for sub in child.children:
                if sub.type in ("identifier", "dotted_name"):
                    imported_names.append(_get_text(sub, source))
                elif sub.type == "aliased_import":
                    id_node = _child_by_type(sub, "identifier")
                    if id_node:
                        imported_names.append(_get_text(id_node, source))
        elif child.type == "aliased_import":
            id_node = _child_by_type(child, "dotted_name") or _child_by_type(child, "identifier")
            if id_node:
                imported_names.append(_get_text(id_node, source))
        elif child.type == "wildcard_import":
            # `from X import *` — skip, log nothing (acceptable)
            pass

    if module_name is None:
        return

    # Resolve the target file path hint for from-imports
    target_file = module_name

    if imported_names:
        for name in imported_names:
            edges.append(
                ParsedEdge(
                    source_name=name,
                    target_name=name,
                    target_file=target_file,
                    relationship="imports",
                )
            )
    else:
        edges.append(
            ParsedEdge(
                source_name=module_name,
                target_name=module_name,
                target_file=target_file,
                relationship="imports",
            )
        )


def _handle_decorated_definition(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    class_stack: list[str],
) -> None:
    """Handle `@decorator\ndef/class ...`."""
    decorator_text = ""
    for child in node.children:
        if child.type == "decorator":
            # Extract decorator name — first identifier or dotted_name
            for sub in child.children:
                if sub.type in ("identifier", "dotted_name", "call"):
                    decorator_text = _get_text(sub, source)
                    break
        elif child.type == "function_definition":
            _handle_function_definition(
                child, source, file_path, symbols, edges, class_stack, context=decorator_text
            )
        elif child.type == "class_definition":
            _handle_class_definition(
                child, source, file_path, symbols, edges, class_stack, context=decorator_text
            )


def _handle_function_definition(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    class_stack: list[str],
    context: str,
) -> None:
    """Extract a function or method symbol."""
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    raw_name = _get_text(name_node, source)

    if class_stack:
        full_name = f"{class_stack[-1]}.{raw_name}"
        kind = "method"
    else:
        full_name = raw_name
        kind = "function"

    ctx = context or _get_context(source, node)
    symbols.append(
        Symbol(
            name=full_name,
            kind=kind,
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="python",
            context=ctx,
        )
    )

    # Extract calls from body — do NOT recurse into body for further symbol discovery
    body = _child_by_type(node, "block")
    if body:
        _extract_calls(body, source, full_name, file_path, edges)


def _handle_class_definition(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    class_stack: list[str],
    context: str,
) -> None:
    """Extract a class symbol and recurse into its body."""
    name_node = _child_by_type(node, "identifier")
    if name_node is None:
        return
    class_name = _get_text(name_node, source)

    ctx = context or _get_context(source, node)
    symbols.append(
        Symbol(
            name=class_name,
            kind="class",
            file=file_path,
            line=node.start_point[0] + 1,
            end_line=node.end_point[0] + 1,
            language="python",
            context=ctx,
        )
    )

    # Extract parent classes from argument_list
    arg_list = _child_by_type(node, "argument_list")
    if arg_list:
        for child in arg_list.children:
            if child.type == "identifier":
                parent_name = _get_text(child, source)
                edges.append(
                    ParsedEdge(
                        source_name=class_name,
                        target_name=parent_name,
                        target_file=None,
                        relationship="extends",
                    )
                )
                edges.append(
                    ParsedEdge(
                        source_name=parent_name,
                        target_name=class_name,
                        target_file=None,
                        relationship="extended_by",
                    )
                )

    # Recurse into body with updated class_stack
    body = _child_by_type(node, "block")
    if body:
        new_stack = class_stack + [class_name]
        for child in body.children:
            _walk_node(child, source, file_path, symbols, edges, new_stack)


def _handle_expression_statement(
    node: Node,
    source: bytes,
    file_path: str,
    symbols: list[Symbol],
) -> None:
    """Capture module-level UPPER_CASE assignments as variables."""
    for child in node.children:
        if child.type == "assignment":
            lhs = child.children[0] if child.children else None
            if lhs is not None and lhs.type == "identifier":
                name = _get_text(lhs, source)
                if name.isupper() and len(name) >= 1:
                    symbols.append(
                        Symbol(
                            name=name,
                            kind="variable",
                            file=file_path,
                            line=node.start_point[0] + 1,
                            end_line=node.end_point[0] + 1,
                            language="python",
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
    """Recursively extract call edges from a function/method body."""
    if node.type == "call":
        func_node = node.children[0] if node.children else None
        if func_node:
            if func_node.type == "identifier":
                callee = _get_text(func_node, source)
                if callee != caller_name:
                    # If capitalized identifier, likely a class instantiation
                    if callee[0].isupper():
                        edges.append(
                            ParsedEdge(
                                source_name=caller_name,
                                target_name=callee,
                                target_file=None,
                                relationship="instantiates",
                            )
                        )
                    else:
                        edges.append(
                            ParsedEdge(
                                source_name=caller_name,
                                target_name=callee,
                                target_file=None,
                                relationship="calls",
                            )
                        )
            elif func_node.type == "attribute":
                # e.g., self.method(), obj.method()
                attr_children = func_node.children
                if len(attr_children) >= 3:
                    obj_node = attr_children[0]
                    method_node = attr_children[-1]
                    obj_text = _get_text(obj_node, source)
                    method_text = _get_text(method_node, source)
                    callee = f"{obj_text}.{method_text}"
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
