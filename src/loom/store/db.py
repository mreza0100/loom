"""SQLite + sqlite-vec database for Loom."""

import logging
import sqlite3
import struct

import sqlite_vec

from loom.config import LoomConfig
from loom.store.models import Edge, Symbol

log = logging.getLogger(__name__)

SCHEMA = """
CREATE TABLE IF NOT EXISTS symbols (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    file TEXT NOT NULL,
    line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    language TEXT NOT NULL,
    context TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file);

DROP TABLE IF EXISTS edges;

CREATE TABLE edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    target_id INTEGER REFERENCES symbols(id) ON DELETE SET NULL,
    target_name TEXT NOT NULL,
    target_file TEXT,
    relationship TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.0,
    original_name TEXT
);

CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
CREATE INDEX IF NOT EXISTS idx_edges_target_name ON edges(target_name);
CREATE INDEX IF NOT EXISTS idx_edges_unresolved ON edges(target_id) WHERE target_id IS NULL;

CREATE TABLE IF NOT EXISTS index_meta (
    file_path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    last_indexed TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
    name, kind, file, context, content=symbols, content_rowid=id
);

CREATE TABLE IF NOT EXISTS cochange (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_a TEXT NOT NULL,
    file_b TEXT NOT NULL,
    frequency INTEGER NOT NULL DEFAULT 1,
    UNIQUE(file_a, file_b)
);
CREATE INDEX IF NOT EXISTS idx_cochange_a ON cochange(file_a);
CREATE INDEX IF NOT EXISTS idx_cochange_b ON cochange(file_b);
"""

VEC_SCHEMA = "CREATE VIRTUAL TABLE IF NOT EXISTS vec_symbols USING vec0(embedding float[{dims}])"


_FTS5_SPECIAL = frozenset({"AND", "OR", "NOT", "NEAR"})


def _sanitize_fts_query(query: str) -> str:
    stripped = query.strip()
    if not stripped:
        return ""
    tokens = stripped.split()
    quoted: list[str] = []
    for token in tokens:
        if token.upper() in _FTS5_SPECIAL or any(c in token for c in '-*"^:'):
            quoted.append(f'"{token}"')
        else:
            quoted.append(token)
    return " ".join(quoted)


def _serialize_vec(vec: list[float]) -> bytes:
    return struct.pack(f"{len(vec)}f", *vec)


class LoomDB:
    def __init__(self, config: LoomConfig) -> None:
        self._config = config
        self._db_path = config.resolve_db_path()
        self._conn: sqlite3.Connection | None = None

    def connect(self) -> None:
        self._conn = sqlite3.connect(str(self._db_path), check_same_thread=False)
        self._conn.execute("PRAGMA journal_mode=WAL")
        self._conn.execute("PRAGMA synchronous=NORMAL")
        self._conn.enable_load_extension(True)
        sqlite_vec.load(self._conn)
        self._conn.enable_load_extension(False)
        # Foreign key enforcement must be enabled per connection — NOT inside executescript
        self._conn.executescript(SCHEMA)
        # Re-apply after executescript (which issues an implicit COMMIT and may reset state)
        self._conn.execute("PRAGMA foreign_keys = ON")
        self._conn.execute(
            VEC_SCHEMA.format(dims=self._config.embedding_dimensions),
        )
        self._conn.commit()
        log.info("Database initialized at %s", self._db_path)

    @property
    def conn(self) -> sqlite3.Connection:
        if self._conn is None:
            raise RuntimeError("Database not connected — call connect() first")
        return self._conn

    def close(self) -> None:
        if self._conn:
            self._conn.close()
            self._conn = None

    # --- File state ---

    def get_file_hash(self, path: str) -> str | None:
        row = self.conn.execute(
            "SELECT content_hash FROM index_meta WHERE file_path = ?",
            (path,),
        ).fetchone()
        return row[0] if row else None

    def set_file_hash(self, path: str, content_hash: str) -> None:
        self.conn.execute(
            "INSERT OR REPLACE INTO index_meta (file_path, content_hash) VALUES (?, ?)",
            (path, content_hash),
        )

    def remove_file(self, path: str) -> None:
        # Step 1: Nullify edges that point TO this file's symbols (convert to unresolved).
        # Must happen BEFORE deleting symbols so we can find them by file.
        self.conn.execute(
            "UPDATE edges SET target_id = NULL, confidence = 0.0 "
            "WHERE target_id IN (SELECT id FROM symbols WHERE file = ?)",
            (path,),
        )
        # Step 2: Delete vectors and FTS entries for this file's symbols
        symbol_ids = [
            row[0]
            for row in self.conn.execute(
                "SELECT id FROM symbols WHERE file = ?",
                (path,),
            ).fetchall()
        ]
        if symbol_ids:
            placeholders = ",".join("?" * len(symbol_ids))
            self.conn.execute(
                f"DELETE FROM vec_symbols WHERE rowid IN ({placeholders})",  # noqa: S608
                symbol_ids,
            )
            self.conn.execute(
                f"DELETE FROM symbols_fts WHERE rowid IN ({placeholders})",  # noqa: S608
                symbol_ids,
            )
        # Step 3: Delete symbols — ON DELETE CASCADE removes edges FROM this file's symbols
        self.conn.execute("DELETE FROM symbols WHERE file = ?", (path,))
        self.conn.execute("DELETE FROM index_meta WHERE file_path = ?", (path,))

    # --- Symbols ---

    def insert_symbol(self, symbol: Symbol) -> int:
        cursor = self.conn.execute(
            "INSERT INTO symbols (name, kind, file, line, end_line, language, context) "
            "VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                symbol.name,
                symbol.kind,
                symbol.file,
                symbol.line,
                symbol.end_line,
                symbol.language,
                symbol.context,
            ),
        )
        symbol_id = cursor.lastrowid
        if symbol_id is None:
            raise RuntimeError("insert_symbol: lastrowid is None — DB returned no rowid")
        self.conn.execute(
            "INSERT INTO symbols_fts (rowid, name, kind, file, context) VALUES (?, ?, ?, ?, ?)",
            (symbol_id, symbol.name, symbol.kind, symbol.file, symbol.context),
        )
        return symbol_id

    def insert_embedding(self, symbol_id: int, embedding: list[float]) -> None:
        self.conn.execute(
            "INSERT INTO vec_symbols (rowid, embedding) VALUES (?, ?)",
            (symbol_id, _serialize_vec(embedding)),
        )

    def insert_edge(self, edge: Edge) -> int:
        cursor = self.conn.execute(
            "INSERT INTO edges "
            "(source_id, target_id, target_name, target_file, relationship, confidence, "
            "original_name) "
            "VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                edge.source_id,
                edge.target_id,
                edge.target_name,
                edge.target_file,
                edge.relationship,
                edge.confidence,
                edge.original_name,
            ),
        )
        row_id = cursor.lastrowid
        if row_id is None:
            raise RuntimeError("insert_edge: lastrowid is None — DB returned no rowid")
        return row_id

    # --- Edge queries ---

    def get_edges_from(self, symbol_id: int) -> list[Edge]:
        rows = self.conn.execute(
            "SELECT id, source_id, target_id, target_name, target_file, relationship, confidence, "
            "original_name FROM edges WHERE source_id = ?",
            (symbol_id,),
        ).fetchall()
        return [self._row_to_edge(r) for r in rows]

    def get_edges_to(self, symbol_id: int) -> list[Edge]:
        rows = self.conn.execute(
            "SELECT id, source_id, target_id, target_name, target_file, relationship, confidence, "
            "original_name FROM edges WHERE target_id = ?",
            (symbol_id,),
        ).fetchall()
        return [self._row_to_edge(r) for r in rows]

    def get_edges_to_by_name(self, target_name: str) -> list[Edge]:
        """Return all edges (resolved and unresolved) where target_name matches.

        Used by impact() to find unresolved callers whose target_id IS NULL but
        target_name matches the queried symbol. Also returns resolved edges with the
        same name — caller must filter out any where target_id != symbol.id.
        """
        rows = self.conn.execute(
            "SELECT id, source_id, target_id, target_name, target_file, relationship, confidence, "
            "original_name FROM edges WHERE target_name = ?",
            (target_name,),
        ).fetchall()
        return [self._row_to_edge(r) for r in rows]

    def get_unresolved_edges(self) -> list[Edge]:
        """Return all edges with target_id IS NULL (pending Phase 2 resolution)."""
        rows = self.conn.execute(
            "SELECT id, source_id, target_id, target_name, target_file, relationship, confidence, "
            "original_name FROM edges WHERE target_id IS NULL",
        ).fetchall()
        return [self._row_to_edge(r) for r in rows]

    def update_edge_target(self, edge_id: int, target_id: int, confidence: float) -> None:
        """Resolve a previously-unresolved edge (Phase 2 resolution)."""
        self.conn.execute(
            "UPDATE edges SET target_id = ?, confidence = ? WHERE id = ?",
            (target_id, confidence, edge_id),
        )

    def remove_edges_for_source(self, symbol_id: int) -> None:
        """Delete all edges where source_id matches. Used when re-indexing a file."""
        self.conn.execute("DELETE FROM edges WHERE source_id = ?", (symbol_id,))

    # --- Queries ---

    def search_fts(self, query: str, limit: int = 20) -> list[Symbol]:
        sanitized = _sanitize_fts_query(query)
        if not sanitized:
            return []
        rows = self.conn.execute(
            "SELECT s.id, s.name, s.kind, s.file, s.line, s.end_line, s.language, s.context "
            "FROM symbols_fts fts "
            "JOIN symbols s ON s.id = fts.rowid "
            "WHERE symbols_fts MATCH ? "
            "ORDER BY rank LIMIT ?",
            (sanitized, limit),
        ).fetchall()
        return [self._row_to_symbol(r) for r in rows]

    def search_vec(self, embedding: list[float], limit: int = 20) -> list[tuple[int, float]]:
        rows = self.conn.execute(
            "SELECT rowid, distance FROM vec_symbols "
            "WHERE embedding MATCH ? AND k = ? ORDER BY distance",
            (_serialize_vec(embedding), limit),
        ).fetchall()
        return [(row[0], row[1]) for row in rows]

    def get_symbol_by_id(self, symbol_id: int) -> Symbol | None:
        row = self.conn.execute(
            "SELECT id, name, kind, file, line, end_line, language, context "
            "FROM symbols WHERE id = ?",
            (symbol_id,),
        ).fetchone()
        return self._row_to_symbol(row) if row else None

    def get_symbol_by_name(self, name: str, file: str | None = None) -> list[Symbol]:
        if file:
            rows = self.conn.execute(
                "SELECT id, name, kind, file, line, end_line, language, context "
                "FROM symbols WHERE name = ? AND file = ?",
                (name, file),
            ).fetchall()
        else:
            rows = self.conn.execute(
                "SELECT id, name, kind, file, line, end_line, language, context "
                "FROM symbols WHERE name = ?",
                (name,),
            ).fetchall()
        return [self._row_to_symbol(r) for r in rows]

    def get_symbol_by_name_fuzzy(self, name: str, file: str | None = None) -> list[Symbol]:
        results = self.get_symbol_by_name(name, file)
        if results:
            return results

        if file:
            file_results = self.get_symbol_by_name(name)
            matched = [
                s for s in file_results if s.file.endswith(file) or s.file.endswith(f"/{file}")
            ]
            if matched:
                return matched

        if "." not in name:
            pattern = f"%.{name}"
            if file:
                rows = self.conn.execute(
                    "SELECT id, name, kind, file, line, end_line, language, context "
                    "FROM symbols WHERE name LIKE ? AND (file = ? OR file LIKE ?)",
                    (pattern, file, f"%/{file}" if "/" not in file else file),
                ).fetchall()
            else:
                rows = self.conn.execute(
                    "SELECT id, name, kind, file, line, end_line, language, context "
                    "FROM symbols WHERE name LIKE ? LIMIT 20",
                    (pattern,),
                ).fetchall()
            if rows:
                return [self._row_to_symbol(r) for r in rows]

        alt_name = name.lstrip("_") if name.startswith("_") else f"_{name}"
        if alt_name != name:
            results = self.get_symbol_by_name(alt_name, file)
            if results:
                return results
            if file:
                file_results = self.get_symbol_by_name(alt_name)
                matched = [
                    s for s in file_results if s.file.endswith(file) or s.file.endswith(f"/{file}")
                ]
                if matched:
                    return matched
            if "." not in alt_name:
                pattern = f"%.{alt_name}"
                rows = self.conn.execute(
                    "SELECT id, name, kind, file, line, end_line, language, context "
                    "FROM symbols WHERE name LIKE ? LIMIT 20",
                    (pattern,),
                ).fetchall()
                if rows:
                    return [self._row_to_symbol(r) for r in rows]

        return []

    def get_colocated_symbols(self, file: str) -> list[Symbol]:
        rows = self.conn.execute(
            "SELECT id, name, kind, file, line, end_line, language, context "
            "FROM symbols WHERE file = ? ORDER BY line",
            (file,),
        ).fetchall()
        return [self._row_to_symbol(r) for r in rows]

    # --- Co-change (evolutionary coupling) ---

    def upsert_cochange(self, file_a: str, file_b: str, frequency: int) -> None:
        """Insert or update a co-change pair.

        Enforces canonical (min, max) ordering at the DB layer regardless of
        argument order — prevents duplicate rows for (a, b) vs (b, a).
        Uses INSERT ... ON CONFLICT DO UPDATE to replace the stored frequency
        with the new authoritative count from the latest full git analysis.
        """
        a, b = (min(file_a, file_b), max(file_a, file_b))
        self.conn.execute(
            "INSERT INTO cochange (file_a, file_b, frequency) VALUES (?, ?, ?) "
            "ON CONFLICT(file_a, file_b) DO UPDATE SET frequency = excluded.frequency",
            (a, b, frequency),
        )

    def get_cochange_frequency(self, file_a: str, file_b: str) -> int:
        """Return stored co-change frequency for a file pair.

        Returns 0 if no row exists (correct default for evolutionary score = 0.0).
        Normalizes to (min, max) ordering before querying.
        """
        a, b = (min(file_a, file_b), max(file_a, file_b))
        row = self.conn.execute(
            "SELECT frequency FROM cochange WHERE file_a = ? AND file_b = ?",
            (a, b),
        ).fetchone()
        return int(row[0]) if row else 0

    def get_top_cochanges(self, file: str, limit: int = 20) -> list[tuple[str, int]]:
        """Return top co-changed partner files for the given file, sorted by frequency desc.

        Uses a CASE expression to return the partner path regardless of which column
        holds the queried file.
        """
        rows = self.conn.execute(
            "SELECT CASE WHEN file_a = ? THEN file_b ELSE file_a END AS other_file, frequency "
            "FROM cochange WHERE file_a = ? OR file_b = ? "
            "ORDER BY frequency DESC LIMIT ?",
            (file, file, file, limit),
        ).fetchall()
        return [(row[0], int(row[1])) for row in rows]

    # --- Stats ---

    def get_stats(self) -> dict[str, int | str | None]:
        symbol_count = self.conn.execute("SELECT COUNT(*) FROM symbols").fetchone()[0]
        edge_count = self.conn.execute("SELECT COUNT(*) FROM edges").fetchone()[0]
        file_count = self.conn.execute("SELECT COUNT(*) FROM index_meta").fetchone()[0]
        vec_count = self.conn.execute("SELECT COUNT(*) FROM vec_symbols").fetchone()[0]
        last_indexed = self.conn.execute(
            "SELECT MAX(last_indexed) FROM index_meta",
        ).fetchone()[0]
        stale_row = self.conn.execute(
            "SELECT COUNT(*) FROM index_meta WHERE last_indexed < datetime('now', '-1 hour')",
        ).fetchone()
        stale_count = stale_row[0] if stale_row else 0
        cochange_count = self.conn.execute("SELECT COUNT(*) FROM cochange").fetchone()[0]
        return {
            "symbols": symbol_count,
            "edges": edge_count,
            "files": file_count,
            "vectors": vec_count,
            "last_indexed": last_indexed,
            "stale_files": stale_count,
            "cochange_pairs": cochange_count,
        }

    def commit(self) -> None:
        self.conn.commit()

    @staticmethod
    def _row_to_symbol(row: tuple) -> Symbol:  # type: ignore[type-arg]
        return Symbol(
            id=row[0],
            name=row[1],
            kind=row[2],
            file=row[3],
            line=row[4],
            end_line=row[5],
            language=row[6],
            context=row[7],
        )

    @staticmethod
    def _row_to_edge(row: tuple) -> Edge:  # type: ignore[type-arg]
        """Construct Edge from a DB row.

        Row column order:
          (id, source_id, target_id, target_name, target_file, relationship, confidence,
           original_name)
        """
        return Edge(
            id=row[0],
            source_id=row[1],
            target_id=row[2],
            target_name=row[3],
            target_file=row[4],
            relationship=row[5],
            confidence=row[6],
            original_name=row[7] if len(row) > 7 else None,
        )
