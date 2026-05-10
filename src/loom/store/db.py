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

CREATE TABLE IF NOT EXISTS edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_name TEXT NOT NULL,
    source_file TEXT NOT NULL,
    target_name TEXT NOT NULL,
    target_file TEXT,
    relationship TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_name, source_file);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_name);

CREATE TABLE IF NOT EXISTS index_meta (
    file_path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    last_indexed TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
    name, kind, file, context, content=symbols, content_rowid=id
);
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
        self._conn.executescript(SCHEMA)
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
        self.conn.execute("DELETE FROM symbols WHERE file = ?", (path,))
        self.conn.execute(
            "DELETE FROM edges WHERE source_file = ? OR target_file = ?",
            (path, path),
        )
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
        assert symbol_id is not None
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

    def insert_edge(self, edge: Edge) -> None:
        self.conn.execute(
            "INSERT INTO edges (source_name, source_file, target_name, target_file, relationship) "
            "VALUES (?, ?, ?, ?, ?)",
            (
                edge.source_name,
                edge.source_file,
                edge.target_name,
                edge.target_file,
                edge.relationship,
            ),
        )

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

    def get_edges_from(self, name: str, file: str | None = None) -> list[Edge]:
        if file:
            rows = self.conn.execute(
                "SELECT source_name, source_file, target_name, target_file, relationship "
                "FROM edges WHERE source_name = ? AND source_file = ?",
                (name, file),
            ).fetchall()
        else:
            rows = self.conn.execute(
                "SELECT source_name, source_file, target_name, target_file, relationship "
                "FROM edges WHERE source_name = ?",
                (name,),
            ).fetchall()
        return [Edge(*r) for r in rows]

    def get_edges_to(self, name: str, file: str | None = None) -> list[Edge]:
        if file:
            rows = self.conn.execute(
                "SELECT source_name, source_file, target_name, target_file, relationship "
                "FROM edges WHERE target_name = ? AND target_file = ?",
                (name, file),
            ).fetchall()
        else:
            rows = self.conn.execute(
                "SELECT source_name, source_file, target_name, target_file, relationship "
                "FROM edges WHERE target_name = ?",
                (name,),
            ).fetchall()
        return [Edge(*r) for r in rows]

    def get_colocated_symbols(self, file: str) -> list[Symbol]:
        rows = self.conn.execute(
            "SELECT id, name, kind, file, line, end_line, language, context "
            "FROM symbols WHERE file = ? ORDER BY line",
            (file,),
        ).fetchall()
        return [self._row_to_symbol(r) for r in rows]

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
        return {
            "symbols": symbol_count,
            "edges": edge_count,
            "files": file_count,
            "vectors": vec_count,
            "last_indexed": last_indexed,
            "stale_files": stale_count,
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
