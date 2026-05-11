"""Indexer pipeline — parses files, extracts symbols, generates embeddings, stores everything.

Two-phase indexing:
  Phase 1 (parse-all): Parse each file, store symbols + raw edges
                       (source_id resolved, target_id=NULL).
  Phase 2 (resolve-all): Build global import map, resolve all unresolved
                         edges with 5-strategy resolution.
"""

import hashlib
import logging
import posixpath
from pathlib import Path

from loom.config import LoomConfig
from loom.indexer.adapters import REGISTRY
from loom.indexer.embedder import Embedder
from loom.indexer.parser import parse_file
from loom.store.db import LoomDB
from loom.store.graph import SymbolGraph
from loom.store.models import Edge

log = logging.getLogger(__name__)


def _hash_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _should_index(path: Path, config: LoomConfig) -> bool:
    if path.suffix not in config.watch_extensions:
        return False
    if any(part in config.excluded_dirs for part in path.parts):
        return False
    return path.stat().st_size <= config.max_file_size_bytes


def _resolve_import_path(import_path: str, source_file: str) -> str:
    source_dir = posixpath.dirname(source_file)
    return posixpath.normpath(posixpath.join(source_dir, import_path))


class IndexPipeline:
    def __init__(
        self,
        config: LoomConfig,
        db: LoomDB,
        embedder: Embedder,
        graph: SymbolGraph | None = None,
    ) -> None:
        self._config = config
        self._db = db
        self._embedder = embedder
        self._graph = graph

    def full_index(self) -> dict[str, int]:
        target = self._config.target_dir
        files = [p for p in target.rglob("*") if p.is_file() and _should_index(p, self._config)]
        log.info("Full index: found %d files to index in %s", len(files), target)
        result = self._parse_all_files(files)
        resolved = self._resolve_all_edges()
        result["resolved"] = resolved

        if self._config.enable_git_analysis:
            from loom.indexer.git_analyzer import GitAnalyzer  # noqa: PLC0415

            git = GitAnalyzer(self._config.target_dir, self._config.watch_extensions)
            if git.is_git_repo():
                cochanges = git.analyze_cochanges(
                    max_commits=self._config.git_max_commits,
                    max_files_per_commit=self._config.git_max_files_per_commit,
                )
                for (file_a, file_b), freq in cochanges.items():
                    self._db.upsert_cochange(file_a, file_b, freq)
                self._db.commit()
                log.info("Git analysis: stored %d co-change pairs", len(cochanges))

        if self._graph is not None:
            self._graph.build_from_db(self._db)
        return result

    def incremental_index(self, changed_paths: list[Path]) -> dict[str, int]:
        files = [p for p in changed_paths if p.exists() and _should_index(p, self._config)]
        deleted = [p for p in changed_paths if not p.exists()]

        for p in deleted:
            rel = str(p.relative_to(self._config.target_dir))
            self._db.remove_file(rel)
            self._db.commit()
            log.info("Removed deleted file from index: %s", rel)

        if not files and not deleted:
            return {"indexed": 0, "deleted": len(deleted), "symbols": 0, "edges": 0, "resolved": 0}

        result = self._parse_all_files(files)
        result["deleted"] = len(deleted)
        # Re-resolve ALL unresolved edges: newly indexed symbols may resolve older unresolved edges
        resolved = self._resolve_all_edges()
        result["resolved"] = resolved
        if self._graph is not None:
            self._graph.build_from_db(self._db)
        return result

    def _parse_all_files(self, files: list[Path]) -> dict[str, int]:
        """Phase 1: parse each file, store symbols + raw edges (target_id=NULL)."""
        total_symbols = 0
        total_edges = 0
        indexed = 0

        for path in files:
            try:
                rel_path = str(path.relative_to(self._config.target_dir))
                content_hash = _hash_file(path)

                existing_hash = self._db.get_file_hash(rel_path)
                if existing_hash == content_hash:
                    continue

                self._db.remove_file(rel_path)

                source = path.read_bytes()
                symbols, parsed_edges = parse_file(path, source)

                symbol_ids: list[int] = []
                embed_texts: list[str] = []

                for sym in symbols:
                    sym.file = rel_path
                    sym_id = self._db.insert_symbol(sym)
                    symbol_ids.append(sym_id)
                    embed_texts.append(
                        self._embedder.build_symbol_text(sym.name, sym.kind, sym.context),
                    )

                if embed_texts:
                    embeddings = self._embedder.embed(embed_texts)
                    for sym_id, emb in zip(symbol_ids, embeddings, strict=True):
                        self._db.insert_embedding(sym_id, emb)

                # Build local name-to-id map (source is always in same file for non-import edges)
                local_name_to_id: dict[str, int] = {
                    sym.name: sym_id for sym, sym_id in zip(symbols, symbol_ids, strict=True)
                }
                # File anchor: any symbol from this file — used for import edges whose
                # local binding name is not itself a declared symbol
                file_anchor_id: int | None = symbol_ids[0] if symbol_ids else None

                # Convert ParsedEdge -> Edge (source_id resolved, target_id=None)
                for parsed in parsed_edges:
                    # For import edges: local_name (source_name) is the imported binding,
                    # not a declared symbol. Use a file anchor symbol as source_id.
                    # Store target_name=local_name so _build_import_map can use it as key.
                    if parsed.relationship == "imports":
                        if file_anchor_id is None:
                            # No symbols in file — can't anchor import edge, skip
                            continue
                        target_file = parsed.target_file
                        if target_file and target_file.startswith("."):
                            target_file = _resolve_import_path(target_file, rel_path)
                        # target_name = LOCAL name (the binding used in code calls)
                        # _build_import_map key: (file, local_name) -> target_file
                        # original_name = exported name in target (differs for aliased imports)
                        local_name = parsed.source_name
                        exported_name = parsed.target_name
                        edge = Edge(
                            source_id=file_anchor_id,
                            target_name=local_name,  # local binding name (import map key)
                            target_file=target_file,
                            relationship="imports",
                            confidence=0.0,
                            target_id=None,
                            # Store original exported name only when it differs from local alias
                            original_name=exported_name if exported_name != local_name else None,
                        )
                        self._db.insert_edge(edge)
                        continue

                    # Non-import edges: source_name is always a declared symbol in this file
                    source_id = local_name_to_id.get(parsed.source_name)
                    if source_id is None:
                        log.debug(
                            "Skipping edge from unknown source '%s' in %s",
                            parsed.source_name,
                            rel_path,
                        )
                        continue

                    edge = Edge(
                        source_id=source_id,
                        target_name=parsed.target_name,
                        target_file=parsed.target_file,
                        relationship=parsed.relationship,
                        confidence=0.0,
                        target_id=None,
                    )
                    self._db.insert_edge(edge)

                self._db.set_file_hash(rel_path, content_hash)

                total_symbols += len(symbols)
                total_edges += len(parsed_edges)
                indexed += 1
                log.info(
                    "Indexed %s: %d symbols, %d edges",
                    rel_path,
                    len(symbols),
                    len(parsed_edges),
                )

            except Exception:
                log.exception("Failed to index %s", path)

        self._db.commit()

        log.info(
            "Parse-all complete: %d files, %d symbols, %d edges",
            indexed,
            total_symbols,
            total_edges,
        )
        return {"indexed": indexed, "symbols": total_symbols, "edges": total_edges}

    def _resolve_all_edges(self) -> int:
        """Phase 2: build global import map, resolve all unresolved edges."""
        import_map: dict[tuple[str, str], tuple[str, str | None]] = self._build_import_map()
        unresolved = self._db.get_unresolved_edges()

        resolved_count = 0
        for edge in unresolved:
            result = self._resolve_single_edge(edge, import_map)
            if result is not None:
                target_id, confidence = result
                if edge.id is None:
                    log.warning("Skipping edge with None id during resolve — data integrity issue")
                    continue
                self._db.update_edge_target(edge.id, target_id, confidence)
                resolved_count += 1

        self._db.commit()
        log.info("Resolve-all: resolved %d of %d unresolved edges", resolved_count, len(unresolved))
        return resolved_count

    def _build_import_map(self) -> dict[tuple[str, str], tuple[str, str | None]]:
        """Build a global map of (source_file, local_name) -> (resolved_target_file, original_name).

        Queries all import edges from the DB. Import edges have:
          - source_id pointing to the importing symbol (in source_file)
          - target_name = local binding name (import map key)
          - target_file = resolved path to the target module (normalized by _parse_all_files)
          - original_name = exported name in target module (differs from local_name for aliases)

        For CommonJS require() paths without extensions (e.g. "./Cache"),
        resolves to actual indexed files by trying common extensions.
        """
        known_files: set[str] = {
            row[0] for row in self._db.conn.execute("SELECT DISTINCT file FROM symbols").fetchall()
        }

        rows = self._db.conn.execute(
            "SELECT e.target_name, s.file, e.target_file, e.original_name "
            "FROM edges e "
            "JOIN symbols s ON s.id = e.source_id "
            "WHERE e.relationship = 'imports' AND e.target_file IS NOT NULL",
        ).fetchall()

        import_map: dict[tuple[str, str], tuple[str, str | None]] = {}
        for local_name, source_file, target_file, original_name in rows:
            resolved = self._resolve_module_file(target_file, known_files, source_file)
            import_map[(source_file, local_name)] = (resolved, original_name)
        return import_map

    def _resolve_module_file(
        self,
        target_file: str,
        known_files: set[str],
        source_file: str,
    ) -> str:
        """Resolve a module path to an actual indexed file.

        Delegates to the adapter registered for source_file's extension.
        Falls back to returning target_file unchanged if no adapter is found.
        """
        ext = Path(source_file).suffix
        adapter = REGISTRY.get_adapter(ext)
        if adapter is not None:
            return adapter.resolve_module_path(target_file, source_file, known_files)
        return target_file

    def _resolve_single_edge(
        self,
        edge: Edge,
        import_map: dict[tuple[str, str], tuple[str, str | None]],
    ) -> tuple[int, float] | None:
        """Try 5 resolution strategies in order. Return (target_id, confidence) or None.

        Strategies (descending confidence):
          1. Exact file match (1.0) — edge.target_file set, symbol found by name+file
          2. Import-resolved (0.95) — first segment of target_name found in import_map;
             for aliased imports, also tries original exported name when local alias fails
          3. File suffix match (0.9) — target_file is a partial path
          4. Qualified name match (0.8) — unique ClassName.method match via LIKE
          5. Unique name match (0.6) — exactly one symbol globally with this name
        """
        target_name = edge.target_name
        target_file = edge.target_file

        # Strategy 1: Exact file + name match
        if target_file:
            candidates = self._db.get_symbol_by_name(target_name, target_file)
            if len(candidates) == 1 and candidates[0].id is not None:
                return (candidates[0].id, 1.0)

        # Get source symbol's file for import map lookup
        source_sym = self._db.get_symbol_by_id(edge.source_id)
        if source_sym is None:
            return None
        source_file = source_sym.file

        # Strategy 2: Import-resolved
        # For dotted expressions: first segment is the import alias
        # For simple names: try the name itself as import alias
        parts = target_name.split(".")
        base = parts[0]  # first segment for dotted, or whole name for simple
        import_entry = import_map.get((source_file, base))

        if import_entry is not None:
            resolved_file, original_name = import_entry
            if len(parts) == 1:
                # Simple name: look for the local binding name directly in the resolved file
                candidates = self._db.get_symbol_by_name(target_name, resolved_file)
                if len(candidates) == 1 and candidates[0].id is not None:
                    return (candidates[0].id, 0.95)
                # Aliased import: local binding not found — try the original exported name
                if original_name and original_name != target_name:
                    candidates = self._db.get_symbol_by_name(original_name, resolved_file)
                    if len(candidates) == 1 and candidates[0].id is not None:
                        return (candidates[0].id, 0.95)
            else:
                # Dotted expression: base is import alias, rest is method/property
                method = ".".join(parts[1:])  # last segment(s) after the alias
                candidates = self._db.get_symbol_by_name(method, resolved_file)
                if not candidates:
                    # Try qualified: ClassName.method
                    candidates = self._db.get_symbol_by_name(f"{base}.{method}", resolved_file)
                if len(candidates) == 1 and candidates[0].id is not None:
                    return (candidates[0].id, 0.95)

        # Strategy 2b: this.X same-file resolution
        # In JS/TS, this.method always refers to a method on the enclosing class
        if base == "this" and len(parts) >= 2:
            method = ".".join(parts[1:])
            # Try ClassName.method in the same file (source symbol's class prefix)
            source_class = source_sym.name.split(".")[0] if "." in source_sym.name else None
            if source_class:
                qualified = f"{source_class}.{method}"
                candidates = self._db.get_symbol_by_name(qualified, source_file)
                if len(candidates) == 1 and candidates[0].id is not None:
                    return (candidates[0].id, 0.95)
            # Fallback: any *.method in same file
            same_file_rows = self._db.conn.execute(
                "SELECT id, name, kind, file, line, end_line, language, context "
                "FROM symbols WHERE file = ? AND name LIKE ?",
                (source_file, f"%.{method}"),
            ).fetchall()
            same_file_matches = [self._db._row_to_symbol(r) for r in same_file_rows]  # noqa: SLF001
            if len(same_file_matches) == 1 and same_file_matches[0].id is not None:
                return (same_file_matches[0].id, 0.9)

        # Strategy 3: File suffix match (target_file is partial path)
        if target_file and import_entry is None:
            # Normalize the suffix for matching
            normalized_suffix = target_file.lstrip("./")
            all_by_name = self._db.get_symbol_by_name(target_name)
            suffix_matches = [
                s
                for s in all_by_name
                if s.file.endswith(normalized_suffix) or s.file.endswith(f"/{normalized_suffix}")
            ]
            if len(suffix_matches) == 1 and suffix_matches[0].id is not None:
                return (suffix_matches[0].id, 0.9)

        # Strategy 4: Qualified name match — look for ClassName.target_name
        # Works for simple names that map to qualified method names
        simple_name = parts[-1]  # last segment
        if simple_name != target_name:
            # Already a dotted expression — check if the full expression exists as a symbol
            candidates = self._db.get_symbol_by_name(target_name)
            if len(candidates) == 1 and candidates[0].id is not None:
                return (candidates[0].id, 0.8)

        # Strategy 4b: Fuzzy qualified — look for *.{simple_name}
        pattern_rows = self._db.conn.execute(
            "SELECT id, name, kind, file, line, end_line, language, context "
            "FROM symbols WHERE name LIKE ?",
            (f"%.{simple_name}",),
        ).fetchall()
        qualified_candidates = [self._db._row_to_symbol(r) for r in pattern_rows]  # noqa: SLF001
        if len(qualified_candidates) == 1 and qualified_candidates[0].id is not None:
            return (qualified_candidates[0].id, 0.8)

        # Strategy 5: Unique name match globally
        # Try the simple name (last segment) for dotted expressions
        global_candidates = self._db.get_symbol_by_name(simple_name)
        if len(global_candidates) == 1 and global_candidates[0].id is not None:
            return (global_candidates[0].id, 0.6)

        # For dotted expressions starting with uppercase: try as ClassName.method (confidence 1.0)
        if len(parts) >= 2 and parts[0][0].isupper():
            candidates = self._db.get_symbol_by_name(target_name)
            if len(candidates) == 1 and candidates[0].id is not None:
                return (candidates[0].id, 1.0)

        return None
