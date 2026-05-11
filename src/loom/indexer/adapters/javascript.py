"""JavaScript/TypeScript language adapter for Loom."""

import logging

import tree_sitter_javascript as tsjs
from tree_sitter import Language, Node, Parser

from loom.store.models import ParsedEdge, Symbol

log = logging.getLogger(__name__)

JS_LANGUAGE = Language(tsjs.language())

_EXTENSION_TO_LANG: dict[str, str] = {
    ".js": "javascript",
    ".jsx": "javascript",
    ".mjs": "javascript",
    ".cjs": "javascript",
    ".ts": "typescript",
    ".tsx": "typescript",
}

_JS_EXTENSIONS: frozenset[str] = frozenset(_EXTENSION_TO_LANG.keys())

_JS_EXCLUDED_DIRS: frozenset[str] = frozenset(
    {"node_modules", "dist", "build", ".next", "coverage"}
)


class JavaScriptAdapter:
    """LanguageAdapter for JavaScript and TypeScript files."""

    extensions: frozenset[str] = _JS_EXTENSIONS
    language_name: str = "javascript"
    excluded_dirs: frozenset[str] = _JS_EXCLUDED_DIRS

    def parse(
        self,
        source: bytes,
        file_path: str,
    ) -> tuple[list[Symbol], list[ParsedEdge]]:
        """Parse JavaScript/TypeScript source bytes and extract symbols and edges."""
        from pathlib import Path  # noqa: PLC0415

        suffix = Path(file_path).suffix
        lang = _EXTENSION_TO_LANG.get(suffix)
        if not lang:
            return [], []

        parser = Parser(JS_LANGUAGE)
        tree = parser.parse(source)
        root = tree.root_node

        symbols: list[Symbol] = []
        edges: list[ParsedEdge] = []

        _walk_node(root, source, file_path, lang, symbols, edges, in_function=False)

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
        """Resolve a module import path to an actual indexed file.

        Tries the path as-is, then with JS/TS extensions, then as index files.
        Returns import_path unchanged if no match found.
        """
        if import_path in known_files:
            return import_path
        for ext in (".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"):
            candidate = import_path + ext
            if candidate in known_files:
                return candidate
        for index in ("index.js", "index.ts"):
            candidate = f"{import_path}/{index}"
            if candidate in known_files:
                return candidate
        return import_path


# ── Module-private helpers (same logic as in the old parser.py) ─────────────


def _get_context(source: bytes, node: Node, max_lines: int = 10) -> str:
    start = node.start_point[0]
    end = min(node.end_point[0] + 1, start + max_lines)
    lines = source.split(b"\n")[start:end]
    return b"\n".join(lines).decode("utf-8", errors="replace")


def _extract_name(node: Node, source: bytes) -> str | None:
    if node.type == "identifier":
        return source[node.start_byte : node.end_byte].decode()
    for child in node.children:
        if child.type == "identifier":
            return source[child.start_byte : child.end_byte].decode()
        if child.type == "property_identifier":
            return source[child.start_byte : child.end_byte].decode()
    return None


def _walk_node(
    node: Node,
    source: bytes,
    file_path: str,
    language: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    *,
    in_function: bool = False,
) -> None:
    if node.type == "function_declaration":
        name = _extract_name(node, source)
        if name:
            symbols.append(
                Symbol(
                    name=name,
                    kind="function",
                    file=file_path,
                    line=node.start_point[0] + 1,
                    end_line=node.end_point[0] + 1,
                    language=language,
                    context=_get_context(source, node),
                ),
            )
            _extract_calls(node, source, name, file_path, edges)
        for child in node.children:
            _walk_node(child, source, file_path, language, symbols, edges, in_function=True)
        return

    if node.type == "class_declaration":
        name = _extract_name(node, source)
        if name:
            symbols.append(
                Symbol(
                    name=name,
                    kind="class",
                    file=file_path,
                    line=node.start_point[0] + 1,
                    end_line=node.end_point[0] + 1,
                    language=language,
                    context=_get_context(source, node),
                ),
            )
            _extract_heritage(node, source, name, file_path, edges)
            _extract_methods(node, source, name, file_path, language, symbols, edges)
        return

    if node.type == "lexical_declaration" or node.type == "variable_declaration":
        for child in node.children:
            if child.type == "variable_declarator":
                _handle_variable_declarator(
                    child,
                    node,
                    source,
                    file_path,
                    language,
                    symbols,
                    edges,
                    in_function=in_function,
                )

    elif node.type == "expression_statement":
        _handle_expression_statement(node, source, file_path, language, symbols, edges)

    elif node.type == "export_statement":
        for child in node.children:
            _walk_node(child, source, file_path, language, symbols, edges, in_function=False)
        return

    elif node.type == "import_statement":
        _handle_import(node, source, file_path, edges)
        return

    for child in node.children:
        _walk_node(child, source, file_path, language, symbols, edges, in_function=in_function)


def _handle_variable_declarator(
    node: Node,
    parent: Node,
    source: bytes,
    file_path: str,
    language: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
    *,
    in_function: bool = False,
) -> None:
    # Check for require() — CommonJS import: const X = require("./path")
    require_info = _extract_require(node, source)
    if require_info:
        _handle_require_declaration(node, source, file_path, require_info, edges)

    name = _extract_name(node, source)
    if not name:
        return

    value = None
    for child in node.children:
        if child.type in ("arrow_function", "function_expression"):
            value = child
            break

    if value:
        kind = "function"
        context_node = value
    else:
        if in_function:
            return
        kind = "variable"
        context_node = parent

    symbols.append(
        Symbol(
            name=name,
            kind=kind,
            file=file_path,
            line=parent.start_point[0] + 1,
            end_line=parent.end_point[0] + 1,
            language=language,
            context=_get_context(source, context_node),
        ),
    )

    if value:
        _extract_calls(value, source, name, file_path, edges)


def _extract_require(node: Node, source: bytes) -> str | None:
    """If node is a variable_declarator with require("..."), return the module path."""
    for child in node.children:
        if child.type == "call_expression":
            func = child.children[0] if child.children else None
            if func and func.type == "identifier":
                func_name = source[func.start_byte : func.end_byte].decode()
                if func_name == "require":
                    args = child.child_by_field_name("arguments")
                    if args is None:
                        for c in child.children:
                            if c.type == "arguments":
                                args = c
                                break
                    if args:
                        for arg in args.children:
                            if arg.type == "string":
                                raw = source[arg.start_byte : arg.end_byte].decode()
                                return raw.strip("'\"")
    return None


def _handle_require_declaration(
    node: Node,
    source: bytes,
    file_path: str,
    module_path: str,
    edges: list[ParsedEdge],
) -> None:
    """Handle const X = require("./path") and const { X, Y } = require("./path")."""
    lhs = node.children[0] if node.children else None
    if lhs is None:
        return

    if lhs.type == "identifier":
        # const X = require("./path") — default import
        name = source[lhs.start_byte : lhs.end_byte].decode()
        edges.append(
            ParsedEdge(
                source_name=name,
                target_name=name,
                target_file=module_path,
                relationship="imports",
            ),
        )
    elif lhs.type == "object_pattern":
        # const { X, Y as Z } = require("./path") — named imports
        for child in lhs.children:
            if child.type == "shorthand_property_identifier_pattern":
                name = source[child.start_byte : child.end_byte].decode()
                edges.append(
                    ParsedEdge(
                        source_name=name,
                        target_name=name,
                        target_file=module_path,
                        relationship="imports",
                    ),
                )
            elif child.type == "pair_pattern":
                # { original: alias }
                ids = [
                    source[c.start_byte : c.end_byte].decode()
                    for c in child.children
                    if c.type in ("identifier", "shorthand_property_identifier_pattern")
                ]
                if len(ids) == 2:
                    edges.append(
                        ParsedEdge(
                            source_name=ids[1],
                            target_name=ids[0],
                            target_file=module_path,
                            relationship="imports",
                        ),
                    )


def _handle_expression_statement(
    node: Node,
    source: bytes,
    file_path: str,
    language: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
) -> None:
    """Handle module.exports.X = expr — CommonJS named exports."""
    for child in node.children:
        if child.type != "assignment_expression":
            continue
        lhs = child.children[0] if child.children else None
        if lhs is None or lhs.type != "member_expression":
            continue
        lhs_text = source[lhs.start_byte : lhs.end_byte].decode()
        if not lhs_text.startswith("module.exports."):
            continue
        export_name = lhs_text[len("module.exports.") :]
        if not export_name.isidentifier():
            continue
        symbols.append(
            Symbol(
                name=export_name,
                kind="variable",
                file=file_path,
                line=node.start_point[0] + 1,
                end_line=node.end_point[0] + 1,
                language=language,
                context=_get_context(source, node),
            ),
        )


def _extract_import_specifier(node: Node, source: bytes) -> tuple[str | None, str | None]:
    identifiers = [
        source[child.start_byte : child.end_byte].decode()
        for child in node.children
        if child.type == "identifier"
    ]
    if len(identifiers) == 2:
        return identifiers[0], identifiers[1]
    if len(identifiers) == 1:
        return identifiers[0], None
    return None, None


def _handle_import(
    node: Node,
    source: bytes,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    source_module = None
    import_pairs: list[tuple[str, str]] = []

    for child in node.children:
        if child.type == "string":
            raw = source[child.start_byte : child.end_byte].decode()
            source_module = raw.strip("'\"")
        elif child.type == "import_clause":
            for sub in child.children:
                if sub.type == "identifier":
                    name = source[sub.start_byte : sub.end_byte].decode()
                    import_pairs.append((name, name))
                elif sub.type == "named_imports":
                    for spec in sub.children:
                        if spec.type == "import_specifier":
                            original, local = _extract_import_specifier(spec, source)
                            if original:
                                import_pairs.append((original, local or original))

    if source_module:
        for original_name, local_name in import_pairs:
            edges.append(
                ParsedEdge(
                    source_name=local_name,
                    target_name=original_name,
                    target_file=source_module,
                    relationship="imports",
                ),
            )


def _extract_calls(
    node: Node,
    source: bytes,
    caller_name: str,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    if node.type == "call_expression":
        func_node = node.children[0] if node.children else None
        if func_node:
            callee = source[func_node.start_byte : func_node.end_byte].decode()
            # Phase 3: store full call expression — no more callee.split(".")[-1]
            # console.* filter operates on raw callee (correct behavior preserved)
            if callee != caller_name and not callee.startswith("console."):
                edges.append(
                    ParsedEdge(
                        source_name=caller_name,
                        target_name=callee,
                        target_file=None,
                        relationship="calls",
                    ),
                )
    elif node.type == "new_expression":
        constructor = node.children[1] if len(node.children) > 1 else None
        if constructor and constructor.type == "identifier":
            class_name = source[constructor.start_byte : constructor.end_byte].decode()
            if class_name != caller_name:
                edges.append(
                    ParsedEdge(
                        source_name=caller_name,
                        target_name=class_name,
                        target_file=None,
                        relationship="instantiates",
                    ),
                )
    for child in node.children:
        _extract_calls(child, source, caller_name, file_path, edges)


def _extract_heritage(
    class_node: Node,
    source: bytes,
    class_name: str,
    file_path: str,
    edges: list[ParsedEdge],
) -> None:
    for child in class_node.children:
        if child.type == "class_heritage":
            for sub in child.children:
                if sub.type == "identifier":
                    parent_name = source[sub.start_byte : sub.end_byte].decode()
                    edges.append(
                        ParsedEdge(
                            source_name=class_name,
                            target_name=parent_name,
                            target_file=None,
                            relationship="extends",
                        ),
                    )
                    edges.append(
                        ParsedEdge(
                            source_name=parent_name,
                            target_name=class_name,
                            target_file=None,
                            relationship="extended_by",
                        ),
                    )


def _extract_methods(
    class_node: Node,
    source: bytes,
    class_name: str,
    file_path: str,
    language: str,
    symbols: list[Symbol],
    edges: list[ParsedEdge],
) -> None:
    body = None
    for child in class_node.children:
        if child.type == "class_body":
            body = child
            break
    if not body:
        return

    for child in body.children:
        if child.type == "method_definition":
            method_name = _extract_name(child, source)
            if method_name:
                full_name = f"{class_name}.{method_name}"
                symbols.append(
                    Symbol(
                        name=full_name,
                        kind="method",
                        file=file_path,
                        line=child.start_point[0] + 1,
                        end_line=child.end_point[0] + 1,
                        language=language,
                        context=_get_context(source, child),
                    ),
                )
                _extract_calls(child, source, full_name, file_path, edges)
