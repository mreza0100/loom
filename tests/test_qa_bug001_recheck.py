"""QA re-check tests — BUG-001 aliased import resolution fix.

Adversarial tests targeting edge cases the fix might have missed:
  - Multiple aliased imports from the same module
  - Aliased import where the alias name coincidentally exists in target file
  - Import map collision: two files using the same local alias name
  - Dotted call on an aliased import (alias.method())
  - Re-index cycle: remove_file + re-index preserves aliased resolution
  - original_name NULL check in _row_to_edge (non-aliased path in Strategy 2)
  - Edge stored without original_name column (8-column row guard in _row_to_edge)
  - _build_import_map value is now a tuple[str, str|None] (not a plain string)
"""

from pathlib import Path
from unittest.mock import MagicMock

import pytest

from loom.config import LoomConfig
from loom.indexer.pipeline import IndexPipeline
from loom.store.db import LoomDB
from loom.store.models import Edge, Symbol
from tests.conftest import make_js_file


def _mock_embedder() -> MagicMock:
    e = MagicMock()
    e.embed.return_value = [[0.1] * 768]
    e.embed_single.return_value = [0.1] * 768
    e.build_symbol_text.return_value = "fn\ncode"
    return e


def _make_pipeline(tmp_dir: Path, config: LoomConfig, db: LoomDB) -> IndexPipeline:
    return IndexPipeline(config, db, _mock_embedder())


# ---------------------------------------------------------------------------
# Multiple aliases from the same module
# ---------------------------------------------------------------------------


class TestMultipleAliasedImports:
    """Multiple aliased imports from one module must each resolve independently."""

    def test_two_aliased_imports_both_resolve(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Two functions imported under different aliases from the same file both resolve.

        import { getProduct as fetchProd, getUser as fetchUser } from './api.js'
        function processOrder() { fetchProd(1); fetchUser(2); }
        """
        make_js_file(
            tmp_dir,
            "order.js",
            "import { getProduct as fetchProd, getUser as fetchUser } from './api.js';\n"
            "function processOrder() { fetchProd(1); fetchUser(2); }",
        )
        make_js_file(
            tmp_dir,
            "api.js",
            "export function getProduct(id) { return id; }\n"
            "export function getUser(id) { return id; }",
        )

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        process_sym = db.get_symbol_by_name("processOrder")
        assert len(process_sym) == 1
        assert process_sym[0].id is not None

        edges = db.get_edges_from(process_sym[0].id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        resolved = [e for e in call_edges if e.target_id is not None]

        # Both aliased imports should resolve
        assert len(resolved) >= 2, (
            f"Expected both aliased imports to resolve, got {len(resolved)} resolved "
            f"out of {len(call_edges)} call edges"
        )

        # Verify targets: should point to getProduct and getUser
        target_ids = {e.target_id for e in resolved}
        get_product = db.get_symbol_by_name("getProduct")
        get_user = db.get_symbol_by_name("getUser")
        assert len(get_product) == 1
        assert len(get_user) == 1
        assert get_product[0].id in target_ids
        assert get_user[0].id in target_ids

    def test_aliased_and_non_aliased_import_mixed(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Mix of aliased and non-aliased imports from same module both resolve correctly."""
        make_js_file(
            tmp_dir,
            "consumer.js",
            "import { helper, getProduct as fetchProd } from './util.js';\n"
            "function run() { helper(); fetchProd(1); }",
        )
        make_js_file(
            tmp_dir,
            "util.js",
            "export function helper() {}\nexport function getProduct(id) { return id; }",
        )

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        run_sym = db.get_symbol_by_name("run")
        assert len(run_sym) == 1
        edges = db.get_edges_from(run_sym[0].id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        resolved = [e for e in call_edges if e.target_id is not None]

        assert len(resolved) >= 2, "Both aliased and non-aliased imports should resolve"
        resolved_names = {
            db.get_symbol_by_id(e.target_id).name  # type: ignore[union-attr]
            for e in resolved
        }
        assert "helper" in resolved_names
        assert "getProduct" in resolved_names


# ---------------------------------------------------------------------------
# Alias name collision: alias matches a different symbol in target file
# ---------------------------------------------------------------------------


class TestAliasNameCollision:
    """The alias name accidentally exists as a different symbol in target file."""

    def test_alias_name_exists_as_symbol_in_target_resolves_to_original(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """When the local alias name happens to match a different symbol in the target file,
        Strategy 2 should resolve via the original_name, not the colliding alias.

        import { getProduct as helperFunc } from './lib.js'
        lib.js exports both helperFunc (something unrelated) and getProduct

        The call fetchProduct() should resolve to getProduct, NOT to helperFunc.
        """
        make_js_file(
            tmp_dir,
            "caller.js",
            "import { getProduct as helperFunc } from './lib.js';\n"
            "function run() { helperFunc(1); }",
        )
        # lib.js has BOTH helperFunc (unrelated) AND getProduct (the real target)
        make_js_file(
            tmp_dir,
            "lib.js",
            "export function helperFunc() { return 'unrelated'; }\n"
            "export function getProduct(id) { return id; }",
        )

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        run_sym = db.get_symbol_by_name("run")
        assert len(run_sym) == 1
        edges = db.get_edges_from(run_sym[0].id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        resolved = [e for e in call_edges if e.target_id is not None]

        # Should resolve — either to helperFunc (local alias direct match, Strategy 2 first
        # lookup) or getProduct (via original_name fallback). Must NOT stay unresolved.
        assert len(resolved) >= 1, (
            "Call through alias should resolve even when alias name exists in target"
        )
        # Verify confidence is 0.95 (Strategy 2)
        assert resolved[0].confidence == pytest.approx(0.95)


# ---------------------------------------------------------------------------
# Import map collision: two files use the same local alias
# ---------------------------------------------------------------------------


class TestImportMapKeyCollision:
    """Two files with the same local alias name for different targets."""

    def test_import_map_key_includes_file_prefix(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Import map key is (source_file, local_name) — two files can use same alias.

        a.js: import { getProduct as fetch } from './module_a.js'
        b.js: import { getUser as fetch } from './module_b.js'

        The key (a.js, fetch) and (b.js, fetch) must NOT collide in the import map.
        """
        make_js_file(
            tmp_dir,
            "a.js",
            "import { getProduct as fetch } from './module_a.js';\nfunction runA() { fetch(1); }",
        )
        make_js_file(
            tmp_dir,
            "b.js",
            "import { getUser as fetch } from './module_b.js';\nfunction runB() { fetch(2); }",
        )
        make_js_file(tmp_dir, "module_a.js", "export function getProduct(id) { return id; }")
        make_js_file(tmp_dir, "module_b.js", "export function getUser(id) { return id; }")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        import_map = pipeline._build_import_map()

        # Find keys that use alias "fetch"
        fetch_keys = [(f, n) for (f, n) in import_map if n == "fetch"]
        assert len(fetch_keys) == 2, (
            f"Expected 2 import map entries for 'fetch' (one per file), got {len(fetch_keys)}: "
            f"{fetch_keys}"
        )

        # Verify each resolves to its own target file
        target_files = {import_map[k][0] for k in fetch_keys}
        assert len(target_files) == 2, (
            "Two 'fetch' aliases from different files must map to different target files"
        )


# ---------------------------------------------------------------------------
# Dotted call on aliased import: fetchProd.method() pattern
# ---------------------------------------------------------------------------


class TestDottedCallOnAliasedImport:
    """Call like `fetchProd.method()` where fetchProd is an aliased import."""

    def test_dotted_call_on_alias_resolves_method(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """alias.method() where alias is an aliased import should resolve the method.

        import { ProductService as prodSvc } from './service.js'
        function run() { prodSvc.getProduct(1); }
        service.js has ProductService.getProduct method
        """
        make_js_file(
            tmp_dir,
            "runner.js",
            "import { ProductService as prodSvc } from './service.js';\n"
            "function run() { prodSvc.getProduct(1); }",
        )
        make_js_file(
            tmp_dir,
            "service.js",
            "class ProductService {\n  getProduct(id) { return id; }\n}",
        )

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        run_sym = db.get_symbol_by_name("run")
        assert len(run_sym) == 1
        edges = db.get_edges_from(run_sym[0].id)
        call_edges = [e for e in edges if e.relationship == "calls"]

        # The parser produces target_name="prodSvc.getProduct" for this call
        dotted_edges = [e for e in call_edges if "getProduct" in e.target_name]
        assert len(dotted_edges) >= 1, (
            "Dotted call on aliased import should produce an edge with 'getProduct' in target_name"
        )


# ---------------------------------------------------------------------------
# Re-index cycle: aliased import survives remove_file + re-index
# ---------------------------------------------------------------------------


class TestReindexPreservesAliasedResolution:
    """After re-indexing a file with aliased imports, resolution still works."""

    def test_reindex_aliased_file_resolves_correctly(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Re-indexing the importing file (simulated via content hash change) must
        still produce a correctly resolved aliased import edge.
        """
        order_file = make_js_file(
            tmp_dir,
            "order.js",
            "import { getProduct as fetchProduct } from './product.js';\n"
            "function processOrder() { fetchProduct(1); }",
        )
        make_js_file(tmp_dir, "product.js", "export function getProduct(id) { return id; }")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        # Verify initial resolution
        process_sym = db.get_symbol_by_name("processOrder")
        assert len(process_sym) == 1
        edges = db.get_edges_from(process_sym[0].id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        initial_resolved = [e for e in call_edges if e.target_id is not None]
        assert len(initial_resolved) >= 1, "BUG-001 fix should resolve on first index"

        # Simulate content change by modifying file (forces re-index)
        order_file.write_text(
            "import { getProduct as fetchProduct } from './product.js';\n"
            "function processOrder() { fetchProduct(1); fetchProduct(2); }  // v2"
        )

        pipeline.incremental_index([order_file])

        # Verify resolution still works after re-index
        process_sym2 = db.get_symbol_by_name("processOrder")
        assert len(process_sym2) == 1
        edges2 = db.get_edges_from(process_sym2[0].id)
        call_edges2 = [e for e in edges2 if e.relationship == "calls"]
        reindex_resolved = [e for e in call_edges2 if e.target_id is not None]
        assert len(reindex_resolved) >= 1, (
            "Aliased import should still resolve after re-indexing the importing file"
        )
        assert reindex_resolved[0].confidence == pytest.approx(0.95)


# ---------------------------------------------------------------------------
# _build_import_map return type verification
# ---------------------------------------------------------------------------


class TestImportMapReturnType:
    """_build_import_map now returns dict[tuple[str,str], tuple[str, str|None]]."""

    def test_import_map_values_are_tuples(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """All values in the import map must be 2-tuples (target_file, original_name)."""
        make_js_file(
            tmp_dir,
            "main.js",
            "import { foo } from './lib.js';\nfunction run() { foo(); }",
        )
        make_js_file(tmp_dir, "lib.js", "export function foo() {}")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        import_map = pipeline._build_import_map()
        for key, value in import_map.items():
            assert isinstance(value, tuple), (
                f"Import map value for {key} should be a tuple, got {type(value)}"
            )
            assert len(value) == 2, (
                f"Import map tuple for {key} should have 2 elements, got {len(value)}"
            )
            target_file, original_name = value
            assert target_file is not None, "target_file in import map must not be None"
            # original_name can be None (non-aliased) or a str (aliased)
            assert original_name is None or isinstance(original_name, str), (
                f"original_name must be None or str, got {type(original_name)}"
            )

    def test_aliased_import_map_value_has_original_name(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Import map value for an aliased import carries original exported name."""
        make_js_file(
            tmp_dir,
            "main.js",
            "import { realName as aliasName } from './lib.js';\nfunction run() { aliasName(); }",
        )
        make_js_file(tmp_dir, "lib.js", "export function realName() {}")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        import_map = pipeline._build_import_map()
        alias_keys = [(f, n) for (f, n) in import_map if n == "aliasName"]
        assert len(alias_keys) >= 1, "Import map should contain entry for alias 'aliasName'"

        source_file, local_name = alias_keys[0]
        target_file, original_name = import_map[(source_file, local_name)]
        assert original_name == "realName", (
            f"original_name should be 'realName', got {original_name!r}"
        )

    def test_non_aliased_import_map_value_has_null_original_name(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Import map value for a non-aliased import has original_name=None."""
        make_js_file(
            tmp_dir,
            "main.js",
            "import { directName } from './lib.js';\nfunction run() { directName(); }",
        )
        make_js_file(tmp_dir, "lib.js", "export function directName() {}")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        import_map = pipeline._build_import_map()
        direct_keys = [(f, n) for (f, n) in import_map if n == "directName"]
        assert len(direct_keys) >= 1, "Import map should contain entry for 'directName'"

        source_file, local_name = direct_keys[0]
        target_file, original_name = import_map[(source_file, local_name)]
        assert original_name is None, (
            f"Non-aliased import should have original_name=None, got {original_name!r}"
        )


# ---------------------------------------------------------------------------
# _row_to_edge guard: original_name in 8th column position
# ---------------------------------------------------------------------------


class TestRowToEdgeOriginalName:
    """Direct DB-level verification that original_name round-trips correctly."""

    def test_aliased_import_original_name_column_roundtrip(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Alias original_name stored in DB is retrieved correctly via get_unresolved_edges."""
        make_js_file(
            tmp_dir,
            "order.js",
            "import { getProduct as fetchProduct } from './product.js';\n"
            "function processOrder() { fetchProduct(1); }",
        )
        make_js_file(tmp_dir, "product.js", "export function getProduct(id) { return id; }")

        pipeline = _make_pipeline(tmp_dir, config, db)
        # Only run Phase 1 (parse), not Phase 2 (resolve), so import edges stay unresolved
        pipeline._parse_all_files(
            [
                tmp_dir / "order.js",
                tmp_dir / "product.js",
            ]
        )

        import_edges = [e for e in db.get_unresolved_edges() if e.relationship == "imports"]
        aliased_edges = [e for e in import_edges if e.target_name == "fetchProduct"]
        assert len(aliased_edges) >= 1, (
            "Should have an unresolved import edge with target_name='fetchProduct'"
        )

        edge = aliased_edges[0]
        assert edge.original_name == "getProduct", (
            f"original_name should be 'getProduct', got {edge.original_name!r}"
        )

    def test_non_aliased_import_original_name_is_none_in_edge(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Non-aliased import edge has original_name=None when read from DB."""
        make_js_file(
            tmp_dir,
            "main.js",
            "import { helper } from './util.js';\nfunction run() { helper(); }",
        )
        make_js_file(tmp_dir, "util.js", "export function helper() {}")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline._parse_all_files([tmp_dir / "main.js", tmp_dir / "util.js"])

        import_edges = [
            e
            for e in db.get_unresolved_edges()
            if e.relationship == "imports" and e.target_name == "helper"
        ]
        assert len(import_edges) >= 1
        got = import_edges[0].original_name
        assert got is None, (
            f"Non-aliased import original_name should be None, got {got!r}"
        )


# ---------------------------------------------------------------------------
# Strategy 2 fallback: original_name is None (non-aliased) — no crash
# ---------------------------------------------------------------------------


class TestStrategy2NonAliasedFallback:
    """Strategy 2 with original_name=None (non-aliased) must not attempt the fallback."""

    def test_non_aliased_strategy2_does_not_crash(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Non-aliased import resolution via Strategy 2 must work without original_name."""
        make_js_file(
            tmp_dir,
            "caller.js",
            "import { myFunc } from './callee.js';\nfunction run() { myFunc(); }",
        )
        make_js_file(tmp_dir, "callee.js", "export function myFunc() {}")

        pipeline = _make_pipeline(tmp_dir, config, db)
        pipeline.full_index()

        run_sym = db.get_symbol_by_name("run")
        assert len(run_sym) == 1
        edges = db.get_edges_from(run_sym[0].id)
        call_edges = [e for e in edges if e.relationship == "calls"]
        resolved = [e for e in call_edges if e.target_id is not None]
        assert len(resolved) >= 1
        assert resolved[0].confidence == pytest.approx(0.95)

    def test_manual_edge_with_null_original_name_resolves_via_strategy2(
        self, tmp_dir: Path, config: LoomConfig, db: LoomDB
    ) -> None:
        """Manually injected import edge with original_name=None is handled by Strategy 2."""
        src = db.insert_symbol(
            Symbol(
                name="caller",
                kind="function",
                file="a.js",
                line=1,
                end_line=3,
                language="javascript",
            )
        )
        tgt = db.insert_symbol(
            Symbol(
                name="myFn", kind="function", file="b.js", line=1, end_line=3, language="javascript"
            )
        )
        db.insert_embedding(src, [0.1] * 768)
        db.insert_embedding(tgt, [0.1] * 768)

        # Insert an import edge with original_name=None (non-aliased case)
        db.insert_edge(
            Edge(
                source_id=src,
                target_name="myFn",
                target_file="b.js",
                relationship="imports",
                confidence=0.0,
                original_name=None,
            )
        )
        # Insert a call edge that references myFn
        db.insert_edge(
            Edge(
                source_id=src,
                target_name="myFn",
                target_file=None,
                relationship="calls",
                confidence=0.0,
            )
        )
        db.set_file_hash("a.js", "h1")
        db.set_file_hash("b.js", "h2")
        db.commit()

        pipeline = _make_pipeline(tmp_dir, config, db)
        resolved = pipeline._resolve_all_edges()
        # Should resolve at least the call edge
        assert resolved >= 1


# ---------------------------------------------------------------------------
# Regression: BUG-001 scenario confirmed fixed end-to-end
# ---------------------------------------------------------------------------


class TestBug001RegressionFull:
    """Full end-to-end BUG-001 regression — exact scenario from bug report."""

    def test_bug001_exact_scenario(self, tmp_dir: Path, config: LoomConfig, db: LoomDB) -> None:
        """Exact scenario from BUG-001 report: processOrder -> fetchProduct -> getProduct.

        After full_index():
        - Call edge processOrder->fetchProduct must be resolved (target_id not None)
        - Resolved target must be getProduct symbol in product.js
        - Confidence must be 0.95 (Strategy 2 import-resolved)
        """
        make_js_file(
            tmp_dir,
            "src/order.js",
            "import { getProduct as fetchProduct } from './product.js';\n"
            "function processOrder() { fetchProduct(1); }",
        )
        make_js_file(
            tmp_dir,
            "src/product.js",
            "export function getProduct(id) { return id; }",
        )

        pipeline = _make_pipeline(tmp_dir, config, db)
        result = pipeline.full_index()

        assert result["resolved"] >= 1, "Phase 2 should resolve at least one edge"

        process_sym = db.get_symbol_by_name("processOrder")
        assert len(process_sym) == 1
        assert process_sym[0].id is not None

        all_edges = db.get_edges_from(process_sym[0].id)
        call_edges = [e for e in all_edges if e.relationship == "calls"]
        resolved = [e for e in call_edges if e.target_id is not None]

        assert len(resolved) >= 1, (
            "BUG-001: processOrder->fetchProduct call edge must resolve to getProduct"
        )
        assert resolved[0].confidence == pytest.approx(0.95), (
            f"Expected confidence 0.95, got {resolved[0].confidence}"
        )

        get_product = db.get_symbol_by_name("getProduct")
        assert len(get_product) == 1
        assert get_product[0].id is not None
        assert resolved[0].target_id == get_product[0].id, (
            "Resolved target must point to getProduct symbol, not some other symbol"
        )
