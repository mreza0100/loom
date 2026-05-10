"""Tests for loom.store.db — SQLite + sqlite-vec database layer."""

import pytest

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

    def test_conn_raises_when_not_connected(self, config: "LoomConfig") -> None:
        from loom.config import LoomConfig

        loom_db = LoomDB(config)
        with pytest.raises(RuntimeError, match="not connected"):
            _ = loom_db.conn

    def test_close_and_reconnect(self, config: "LoomConfig") -> None:
        from loom.config import LoomConfig

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
            )
        )
        db.insert_symbol(
            Symbol(
                name="getProduct",
                kind="function",
                file="b.js",
                line=1,
                end_line=5,
                language="javascript",
            )
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
            )
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
            )
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
            )
        )
        db.insert_symbol(
            Symbol(
                name="Other.compile",
                kind="method",
                file="Other.js",
                line=10,
                end_line=20,
                language="javascript",
            )
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
            )
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
            )
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
            )
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
            )
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
            )
        )
        db.insert_symbol(
            Symbol(
                name="funcB",
                kind="function",
                file="mod.js",
                line=10,
                end_line=15,
                language="javascript",
            )
        )
        db.insert_symbol(
            Symbol(
                name="funcC",
                kind="function",
                file="other.js",
                line=1,
                end_line=5,
                language="javascript",
            )
        )
        db.commit()

        colocated = db.get_colocated_symbols("mod.js")
        assert len(colocated) == 2
        assert colocated[0].name == "funcA"
        assert colocated[1].name == "funcB"

    def test_get_colocated_symbols_empty(self, db: LoomDB) -> None:
        assert db.get_colocated_symbols("nonexistent.js") == []


class TestEdgeCRUD:
    def test_insert_and_query_from(self, db: LoomDB, sample_edge: Edge) -> None:
        db.insert_edge(sample_edge)
        db.commit()

        edges = db.get_edges_from("processOrder")
        assert len(edges) == 1
        assert edges[0].target_name == "validateCart"
        assert edges[0].relationship == "calls"

    def test_get_edges_from_with_file(self, db: LoomDB) -> None:
        db.insert_edge(
            Edge(
                source_name="func",
                source_file="a.js",
                target_name="helper",
                target_file=None,
                relationship="calls",
            )
        )
        db.insert_edge(
            Edge(
                source_name="func",
                source_file="b.js",
                target_name="other",
                target_file=None,
                relationship="calls",
            )
        )
        db.commit()

        all_edges = db.get_edges_from("func")
        assert len(all_edges) == 2

        filtered = db.get_edges_from("func", file="a.js")
        assert len(filtered) == 1
        assert filtered[0].target_name == "helper"

    def test_get_edges_to(self, db: LoomDB, sample_edge: Edge) -> None:
        db.insert_edge(sample_edge)
        db.commit()

        edges = db.get_edges_to("validateCart")
        assert len(edges) == 1
        assert edges[0].source_name == "processOrder"

    def test_get_edges_to_with_file(self, db: LoomDB) -> None:
        db.insert_edge(
            Edge(
                source_name="caller1",
                source_file="a.js",
                target_name="target",
                target_file="t.js",
                relationship="calls",
            )
        )
        db.insert_edge(
            Edge(
                source_name="caller2",
                source_file="b.js",
                target_name="target",
                target_file=None,
                relationship="calls",
            )
        )
        db.commit()

        with_file = db.get_edges_to("target", file="t.js")
        assert len(with_file) == 1  # only explicit file match, not NULL

        without_file = db.get_edges_to("target")
        assert len(without_file) == 2

    def test_no_edges(self, db: LoomDB) -> None:
        assert db.get_edges_from("nonexistent") == []
        assert db.get_edges_to("nonexistent") == []


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
                )
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
            )
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
            )
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
            )
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
                )
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
        sym_id = db.insert_symbol(
            Symbol(
                name="doStuff",
                kind="function",
                file="remove_me.js",
                line=1,
                end_line=5,
                language="javascript",
            )
        )
        db.insert_embedding(sym_id, [0.1] * 768)
        db.insert_edge(
            Edge(
                source_name="doStuff",
                source_file="remove_me.js",
                target_name="helper",
                target_file="other.js",
                relationship="calls",
            )
        )
        db.insert_edge(
            Edge(
                source_name="caller",
                source_file="other.js",
                target_name="doStuff",
                target_file="remove_me.js",
                relationship="calls",
            )
        )
        db.set_file_hash("remove_me.js", "hash1")
        db.commit()

        db.remove_file("remove_me.js")
        db.commit()

        assert db.get_symbol_by_name("doStuff") == []
        assert db.get_file_hash("remove_me.js") is None
        assert db.get_edges_from("doStuff", file="remove_me.js") == []
        assert db.search_vec([0.1] * 768, limit=5) == []

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
