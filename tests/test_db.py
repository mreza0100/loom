"""Tests for loom.store.db — SQLite + sqlite-vec database layer."""

import pytest

from loom.config import LoomConfig
from loom.store.db import LoomDB, _sanitize_fts_query, _serialize_vec
from loom.store.models import Edge, Symbol


class TestSanitizeFtsQuery:
    def test_empty_string(self) -> None:
        assert _sanitize_fts_query("") == ""

    def test_whitespace_only(self) -> None:
        assert _sanitize_fts_query("   ") == ""

    def test_simple_query(self) -> None:
        assert _sanitize_fts_query("processOrder") == "processOrder"

    def test_multi_word_query(self) -> None:
        assert _sanitize_fts_query("process order") == "process order"

    def test_and_operator_quoted(self) -> None:
        result = _sanitize_fts_query("this AND that")
        assert '"AND"' in result

    def test_or_operator_quoted(self) -> None:
        result = _sanitize_fts_query("this OR that")
        assert '"OR"' in result

    def test_not_operator_quoted(self) -> None:
        result = _sanitize_fts_query("NOT this")
        assert '"NOT"' in result

    def test_near_operator_quoted(self) -> None:
        result = _sanitize_fts_query("NEAR something")
        assert '"NEAR"' in result

    def test_hyphen_quoted(self) -> None:
        result = _sanitize_fts_query("not-really-js")
        assert '"not-really-js"' in result

    def test_asterisk_quoted(self) -> None:
        result = _sanitize_fts_query("test*")
        assert '"test*"' in result

    def test_double_quote_quoted(self) -> None:
        result = _sanitize_fts_query('"exact"')
        assert result.count('"') >= 2

    def test_caret_quoted(self) -> None:
        result = _sanitize_fts_query("^start")
        assert '"^start"' in result

    def test_colon_quoted(self) -> None:
        result = _sanitize_fts_query("name:value")
        assert '"name:value"' in result

    def test_mixed_special_and_normal(self) -> None:
        result = _sanitize_fts_query("hello AND world-wide")
        assert "hello" in result
        assert '"AND"' in result
        assert '"world-wide"' in result

    def test_case_insensitive_operators(self) -> None:
        assert '"and"' in _sanitize_fts_query("and")
        assert '"And"' in _sanitize_fts_query("And")


class TestSerializeVec:
    def test_roundtrip(self) -> None:
        import struct

        vec = [1.0, 2.0, 3.0]
        serialized = _serialize_vec(vec)
        result = list(struct.unpack(f"{len(vec)}f", serialized))
        assert result == pytest.approx(vec)

    def test_empty_vec(self) -> None:
        assert _serialize_vec([]) == b""

    def test_length(self) -> None:
        vec = [0.0] * 768
        assert len(_serialize_vec(vec)) == 768 * 4


class TestLoomDBConnection:
    def test_connect_creates_tables(self, db: LoomDB) -> None:
        tables = db.conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        ).fetchall()
        table_names = {row[0] for row in tables}
        assert "symbols" in table_names
        assert "edges" in table_names
        assert "index_meta" in table_names
        assert "vec_symbols" in table_names

    def test_conn_raises_when_not_connected(self, config: LoomConfig) -> None:
        loom_db = LoomDB(config)
        with pytest.raises(RuntimeError, match="not connected"):
            _ = loom_db.conn

    def test_close_and_reconnect(self, config: LoomConfig) -> None:
        loom_db = LoomDB(config)
        loom_db.connect()
        loom_db.close()
        with pytest.raises(RuntimeError):
            _ = loom_db.conn
        loom_db.connect()
        assert loom_db.conn is not None
        loom_db.close()

    def test_close_idempotent(self, db: LoomDB) -> None:
        db.close()
        db.close()

    def test_foreign_keys_enabled(self, db: LoomDB) -> None:
        result = db.conn.execute("PRAGMA foreign_keys").fetchone()
        assert result[0] == 1


class TestSymbolCRUD:
    def test_insert_and_retrieve(self, db: LoomDB, sample_symbol: Symbol) -> None:
        sym_id = db.insert_symbol(sample_symbol)
        assert sym_id > 0

        result = db.get_symbol_by_id(sym_id)
        assert result is not None
        assert result.name == "processOrder"
        assert result.kind == "function"
        assert result.file == "src/services/order.js"
        assert result.line == 10
        assert result.end_line == 25

    def test_get_nonexistent_symbol(self, db: LoomDB) -> None:
        assert db.get_symbol_by_id(99999) is None

    def test_get_symbol_by_name(self, db: LoomDB, sample_symbol: Symbol) -> None:
        db.insert_symbol(sample_symbol)
        db.commit()

        results = db.get_symbol_by_name("processOrder")
        assert len(results) == 1
        assert results[0].name == "processOrder"

    def test_get_symbol_by_name_with_file(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="getProduct",
                kind="function",
                file="a.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        db.insert_symbol(
            Symbol(
                name="getProduct",
                kind="function",
                file="b.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        db.commit()

        all_results = db.get_symbol_by_name("getProduct")
        assert len(all_results) == 2

        filtered = db.get_symbol_by_name("getProduct", file="a.js")
        assert len(filtered) == 1
        assert filtered[0].file == "a.js"

    def test_get_symbol_by_name_not_found(self, db: LoomDB) -> None:
        assert db.get_symbol_by_name("nonexistent") == []


class TestSymbolFuzzyLookup:
    def test_exact_match_preferred(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="compile",
                kind="function",
                file="a.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        db.commit()
        results = db.get_symbol_by_name_fuzzy("compile")
        assert len(results) == 1
        assert results[0].name == "compile"

    def test_method_suffix_match(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="Compiler.compile",
                kind="method",
                file="Compiler.js",
                line=100,
                end_line=150,
                language="javascript",
            ),
        )
        db.commit()
        results = db.get_symbol_by_name_fuzzy("compile")
        assert len(results) == 1
        assert results[0].name == "Compiler.compile"

    def test_method_suffix_with_file(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="Compiler.compile",
                kind="method",
                file="Compiler.js",
                line=100,
                end_line=150,
                language="javascript",
            ),
        )
        db.insert_symbol(
            Symbol(
                name="Other.compile",
                kind="method",
                file="Other.js",
                line=10,
                end_line=20,
                language="javascript",
            ),
        )
        db.commit()
        results = db.get_symbol_by_name_fuzzy("compile", file="Compiler.js")
        assert len(results) == 1
        assert results[0].name == "Compiler.compile"

    def test_underscore_prefix_toggle(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="_makePathsRelative",
                kind="function",
                file="util.js",
                line=1,
                end_line=10,
                language="javascript",
            ),
        )
        db.commit()
        results = db.get_symbol_by_name_fuzzy("makePathsRelative")
        assert len(results) == 1
        assert results[0].name == "_makePathsRelative"

    def test_underscore_prefix_strip(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="publicFunc",
                kind="function",
                file="mod.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        db.commit()
        results = db.get_symbol_by_name_fuzzy("_publicFunc")
        assert len(results) == 1
        assert results[0].name == "publicFunc"

    def test_file_suffix_match(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="helper",
                kind="function",
                file="lib/utils/helper.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        db.commit()
        results = db.get_symbol_by_name_fuzzy("helper", file="helper.js")
        assert len(results) == 1
        assert results[0].file == "lib/utils/helper.js"

    def test_no_match_returns_empty(self, db: LoomDB) -> None:
        assert db.get_symbol_by_name_fuzzy("totallyFake") == []

    def test_does_not_match_substring(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="recompile",
                kind="function",
                file="a.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        db.commit()
        results = db.get_symbol_by_name_fuzzy("compile")
        assert len(results) == 0

    def test_get_colocated_symbols(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="funcA",
                kind="function",
                file="mod.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        db.insert_symbol(
            Symbol(
                name="funcB",
                kind="function",
                file="mod.js",
                line=10,
                end_line=15,
                language="javascript",
            ),
        )
        db.insert_symbol(
            Symbol(
                name="funcC",
                kind="function",
                file="other.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        db.commit()

        colocated = db.get_colocated_symbols("mod.js")
        assert len(colocated) == 2
        assert colocated[0].name == "funcA"
        assert colocated[1].name == "funcB"

    def test_get_colocated_symbols_empty(self, db: LoomDB) -> None:
        assert db.get_colocated_symbols("nonexistent.js") == []


class TestEdgeCRUD:
    def _make_sym(self, db: LoomDB, name: str, file: str = "a.js") -> int:
        return db.insert_symbol(
            Symbol(
                name=name, kind="function", file=file, line=1, end_line=5, language="javascript"
            ),
        )

    def test_insert_and_query_from(self, db: LoomDB) -> None:
        src_id = self._make_sym(db, "processOrder", "order.js")
        tgt_id = self._make_sym(db, "validateCart", "validation.js")
        db.insert_edge(
            Edge(
                source_id=src_id,
                target_name="validateCart",
                target_id=tgt_id,
                relationship="calls",
                confidence=1.0,
            ),
        )
        db.commit()

        edges = db.get_edges_from(src_id)
        assert len(edges) == 1
        assert edges[0].target_name == "validateCart"
        assert edges[0].relationship == "calls"
        assert edges[0].target_id == tgt_id

    def test_get_edges_from_empty(self, db: LoomDB) -> None:
        src_id = self._make_sym(db, "lonely")
        db.commit()
        assert db.get_edges_from(src_id) == []

    def test_get_edges_to(self, db: LoomDB) -> None:
        src_id = self._make_sym(db, "processOrder", "order.js")
        tgt_id = self._make_sym(db, "validateCart", "validation.js")
        db.insert_edge(
            Edge(
                source_id=src_id, target_name="validateCart", target_id=tgt_id, relationship="calls"
            ),
        )
        db.commit()

        edges = db.get_edges_to(tgt_id)
        assert len(edges) == 1
        assert edges[0].source_id == src_id

    def test_no_edges(self, db: LoomDB) -> None:
        sym_id = self._make_sym(db, "nonexistent")
        db.commit()
        assert db.get_edges_from(sym_id) == []
        assert db.get_edges_to(sym_id) == []

    def test_insert_returns_edge_id(self, db: LoomDB) -> None:
        src_id = self._make_sym(db, "src")
        edge_id = db.insert_edge(
            Edge(source_id=src_id, target_name="foo", relationship="calls"),
        )
        assert edge_id > 0

    def test_get_unresolved_edges(self, db: LoomDB) -> None:
        src_id = self._make_sym(db, "caller")
        db.insert_edge(Edge(source_id=src_id, target_name="unknownTarget", relationship="calls"))
        db.commit()

        unresolved = db.get_unresolved_edges()
        assert len(unresolved) >= 1
        assert any(e.target_name == "unknownTarget" for e in unresolved)
        assert all(e.target_id is None for e in unresolved)

    def test_update_edge_target(self, db: LoomDB) -> None:
        src_id = self._make_sym(db, "caller")
        tgt_id = self._make_sym(db, "callee")
        edge_id = db.insert_edge(
            Edge(source_id=src_id, target_name="callee", relationship="calls"),
        )
        db.commit()

        db.update_edge_target(edge_id, tgt_id, 0.95)
        db.commit()

        edges = db.get_edges_from(src_id)
        assert len(edges) == 1
        assert edges[0].target_id == tgt_id
        assert edges[0].confidence == pytest.approx(0.95)

    def test_get_edges_to_by_name(self, db: LoomDB) -> None:
        src_id = self._make_sym(db, "caller")
        db.insert_edge(Edge(source_id=src_id, target_name="fooBar", relationship="calls"))
        db.commit()

        results = db.get_edges_to_by_name("fooBar")
        assert len(results) == 1
        assert results[0].target_name == "fooBar"

    def test_edge_confidence_roundtrip(self, db: LoomDB) -> None:
        src_id = self._make_sym(db, "src")
        tgt_id = self._make_sym(db, "tgt")
        db.insert_edge(
            Edge(
                source_id=src_id,
                target_name="tgt",
                target_id=tgt_id,
                relationship="calls",
                confidence=0.85,
            ),
        )
        db.commit()

        edges = db.get_edges_from(src_id)
        assert len(edges) == 1
        assert edges[0].confidence == pytest.approx(0.85)

    def test_remove_file_nullifies_target_edges(self, db: LoomDB) -> None:
        """Removing a file should nullify edges pointing TO its symbols, not delete them."""
        # File A has a symbol that other files point to
        tgt_id = db.insert_symbol(
            Symbol(
                name="targetFunc",
                kind="function",
                file="a.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        # File B has a caller pointing to targetFunc in a.js
        src_id = db.insert_symbol(
            Symbol(
                name="caller",
                kind="function",
                file="b.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        edge_id = db.insert_edge(
            Edge(
                source_id=src_id,
                target_name="targetFunc",
                target_id=tgt_id,
                relationship="calls",
                confidence=1.0,
            ),
        )
        db.commit()

        # Remove file A
        db.remove_file("a.js")
        db.commit()

        # Edge from b.js should still exist but with target_id=NULL
        unresolved = db.get_unresolved_edges()
        unresolved_ids = [e.id for e in unresolved]
        assert edge_id in unresolved_ids

        # targetFunc symbol should be gone
        assert db.get_symbol_by_name("targetFunc") == []

    def test_remove_edges_for_source(self, db: LoomDB) -> None:
        src_id = self._make_sym(db, "src")
        tgt_id = self._make_sym(db, "tgt")
        db.insert_edge(
            Edge(source_id=src_id, target_name="tgt", target_id=tgt_id, relationship="calls"),
        )
        db.commit()

        db.remove_edges_for_source(src_id)
        db.commit()

        assert db.get_edges_from(src_id) == []


class TestEmbedding:
    def test_insert_and_search(self, db: LoomDB, sample_symbol: Symbol) -> None:
        sym_id = db.insert_symbol(sample_symbol)
        embedding = [0.5] * 768
        db.insert_embedding(sym_id, embedding)
        db.commit()

        results = db.search_vec(embedding, limit=5)
        assert len(results) == 1
        assert results[0][0] == sym_id
        assert results[0][1] == pytest.approx(0.0, abs=0.01)

    def test_search_vec_empty(self, db: LoomDB) -> None:
        results = db.search_vec([0.0] * 768, limit=5)
        assert results == []

    def test_search_vec_respects_limit(self, db: LoomDB) -> None:
        for i in range(5):
            sym_id = db.insert_symbol(
                Symbol(
                    name=f"func{i}",
                    kind="function",
                    file="f.js",
                    line=i,
                    end_line=i + 1,
                    language="javascript",
                ),
            )
            db.insert_embedding(sym_id, [float(i) / 10] * 768)
        db.commit()

        results = db.search_vec([0.0] * 768, limit=2)
        assert len(results) == 2


class TestFTS:
    def test_search_fts_basic(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="processOrder",
                kind="function",
                file="order.js",
                line=1,
                end_line=10,
                language="javascript",
                context="function processOrder(cart) { }",
            ),
        )
        db.commit()

        results = db.search_fts("processOrder")
        assert len(results) == 1
        assert results[0].name == "processOrder"

    def test_search_fts_context_match(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="helper",
                kind="function",
                file="h.js",
                line=1,
                end_line=5,
                language="javascript",
                context="function helper() { validate the cart items }",
            ),
        )
        db.commit()

        results = db.search_fts("cart")
        assert len(results) == 1

    def test_search_fts_empty_query(self, db: LoomDB) -> None:
        assert db.search_fts("") == []
        assert db.search_fts("   ") == []

    def test_search_fts_no_results(self, db: LoomDB) -> None:
        assert db.search_fts("xyzzy_nonexistent") == []

    def test_search_fts_special_chars(self, db: LoomDB) -> None:
        db.insert_symbol(
            Symbol(
                name="myFunc",
                kind="function",
                file="f.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        db.commit()
        results = db.search_fts("my-func AND something")
        assert isinstance(results, list)

    def test_search_fts_respects_limit(self, db: LoomDB) -> None:
        for i in range(10):
            db.insert_symbol(
                Symbol(
                    name=f"func{i}",
                    kind="function",
                    file="f.js",
                    line=i,
                    end_line=i + 1,
                    language="javascript",
                    context=f"function func{i}() {{ common code }}",
                ),
            )
        db.commit()

        results = db.search_fts("common", limit=3)
        assert len(results) <= 3


class TestFileState:
    def test_set_and_get_hash(self, db: LoomDB) -> None:
        db.set_file_hash("src/app.js", "abc123")
        db.commit()
        assert db.get_file_hash("src/app.js") == "abc123"

    def test_get_hash_nonexistent(self, db: LoomDB) -> None:
        assert db.get_file_hash("nonexistent.js") is None

    def test_update_hash(self, db: LoomDB) -> None:
        db.set_file_hash("src/app.js", "hash1")
        db.commit()
        db.set_file_hash("src/app.js", "hash2")
        db.commit()
        assert db.get_file_hash("src/app.js") == "hash2"


class TestRemoveFile:
    def test_remove_file_cleans_everything(self, db: LoomDB) -> None:
        """Removing a file deletes its symbols (CASCADE removes its outgoing edges)
        and nullifies incoming edges (target_id -> NULL)."""
        # File A: has a symbol and a source edge (to b.js)
        src_id = db.insert_symbol(
            Symbol(
                name="doStuff",
                kind="function",
                file="remove_me.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )
        db.insert_embedding(src_id, [0.1] * 768)

        # File B: has a symbol that remove_me.js points to
        tgt_id = db.insert_symbol(
            Symbol(
                name="helper",
                kind="function",
                file="other.js",
                line=1,
                end_line=5,
                language="javascript",
            ),
        )

        # Edge from doStuff -> helper (should be CASCADE deleted when remove_me.js is removed)
        outgoing_edge_id = db.insert_edge(
            Edge(
                source_id=src_id,
                target_name="helper",
                target_id=tgt_id,
                relationship="calls",
                confidence=1.0,
            ),
        )

        # File B also has a caller -> doStuff (this edge should become unresolved)
        caller_id = db.insert_symbol(
            Symbol(
                name="caller",
                kind="function",
                file="other.js",
                line=10,
                end_line=15,
                language="javascript",
            ),
        )
        incoming_edge_id = db.insert_edge(
            Edge(
                source_id=caller_id,
                target_name="doStuff",
                target_id=src_id,
                relationship="calls",
                confidence=1.0,
            ),
        )
        db.set_file_hash("remove_me.js", "hash1")
        db.commit()

        db.remove_file("remove_me.js")
        db.commit()

        # doStuff symbol should be gone
        assert db.get_symbol_by_name("doStuff") == []
        # File hash should be gone
        assert db.get_file_hash("remove_me.js") is None
        # Vector for doStuff should be gone
        assert db.search_vec([0.1] * 768, limit=5) == []

        # Outgoing edge (doStuff -> helper) should be CASCADE deleted
        outgoing = db.conn.execute(
            "SELECT id FROM edges WHERE id = ?", (outgoing_edge_id,)
        ).fetchone()
        assert outgoing is None

        # Incoming edge (caller -> doStuff) should now be unresolved (target_id=NULL)
        incoming_row = db.conn.execute(
            "SELECT id, target_id FROM edges WHERE id = ?",
            (incoming_edge_id,),
        ).fetchone()
        assert incoming_row is not None
        assert incoming_row[1] is None  # target_id nullified

    def test_remove_nonexistent_file(self, db: LoomDB) -> None:
        db.remove_file("nonexistent.js")


class TestStats:
    def test_empty_stats(self, db: LoomDB) -> None:
        stats = db.get_stats()
        assert stats["symbols"] == 0
        assert stats["edges"] == 0
        assert stats["files"] == 0
        assert stats["vectors"] == 0
        assert stats["last_indexed"] is None

    def test_stats_after_inserts(self, populated_db: LoomDB) -> None:
        stats = populated_db.get_stats()
        assert stats["symbols"] == 7
        assert stats["edges"] == 4
        assert stats["files"] == 6
        assert stats["vectors"] == 7
