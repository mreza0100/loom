"""Shared fixtures for Loom test suite."""

from pathlib import Path

import pytest

from loom.config import LoomConfig
from loom.store.db import LoomDB
from loom.store.models import Edge, Symbol


@pytest.fixture
def tmp_dir(tmp_path: Path) -> Path:
    return tmp_path


@pytest.fixture
def config(tmp_dir: Path) -> LoomConfig:
    return LoomConfig(target_dir=tmp_dir)


@pytest.fixture
def db(config: LoomConfig) -> LoomDB:
    loom_db = LoomDB(config)
    loom_db.connect()
    yield loom_db  # type: ignore[misc]
    loom_db.close()


@pytest.fixture
def sample_symbol() -> Symbol:
    return Symbol(
        name="processOrder",
        kind="function",
        file="src/services/order.js",
        line=10,
        end_line=25,
        language="javascript",
        context="function processOrder(cart, user) {\n  // ...\n}",
    )


@pytest.fixture
def populated_db(db: LoomDB) -> LoomDB:
    """Populate DB with test symbols and ID-based edges."""
    symbols = [
        Symbol(
            name="processOrder",
            kind="function",
            file="src/services/order.js",
            line=10,
            end_line=25,
            language="javascript",
            context="function processOrder(cart, user) { return validateCart(cart); }",
        ),
        Symbol(
            name="validateCart",
            kind="function",
            file="src/utils/validation.js",
            line=1,
            end_line=10,
            language="javascript",
            context="function validateCart(cart) { return cart.items.length > 0; }",
        ),
        Symbol(
            name="Cart",
            kind="class",
            file="src/models/cart.js",
            line=1,
            end_line=50,
            language="javascript",
            context="class Cart { constructor() { this.items = []; } }",
        ),
        Symbol(
            name="Cart.addItem",
            kind="method",
            file="src/models/cart.js",
            line=10,
            end_line=20,
            language="javascript",
            context="addItem(product) { this.items.push(product); }",
        ),
        Symbol(
            name="getProduct",
            kind="function",
            file="src/services/product.js",
            line=1,
            end_line=8,
            language="javascript",
            context="function getProduct(id) { return db.find(id); }",
        ),
        Symbol(
            name="getProduct",
            kind="function",
            file="src/services/inventory.js",
            line=1,
            end_line=5,
            language="javascript",
            context="function getProduct(id) { return warehouse.lookup(id); }",
        ),
        Symbol(
            name="MAX_ITEMS",
            kind="variable",
            file="src/config.js",
            line=1,
            end_line=1,
            language="javascript",
            context="const MAX_ITEMS = 100;",
        ),
    ]

    embedding = [0.1] * 768
    ids: list[int] = []
    for sym in symbols:
        sym_id = db.insert_symbol(sym)
        ids.append(sym_id)
        db.insert_embedding(sym_id, embedding)

    # ids[0]=processOrder, ids[1]=validateCart, ids[2]=Cart, ids[3]=Cart.addItem
    # ids[4]=getProduct(product.js), ids[5]=getProduct(inventory.js), ids[6]=MAX_ITEMS
    edges = [
        Edge(
            source_id=ids[0],
            target_name="validateCart",
            target_id=ids[1],
            relationship="calls",
            confidence=1.0,
        ),
        Edge(
            source_id=ids[0],
            target_name="Cart",
            target_id=ids[2],
            relationship="instantiates",
            confidence=1.0,
        ),
        Edge(
            source_id=ids[0],
            target_name="getProduct",
            target_id=ids[4],
            relationship="calls",
            confidence=1.0,
        ),
        Edge(
            source_id=ids[2],
            target_name="EventEmitter",
            target_id=None,
            relationship="extends",
            confidence=0.0,
        ),
    ]
    for edge in edges:
        db.insert_edge(edge)

    db.set_file_hash("src/services/order.js", "hash_order")
    db.set_file_hash("src/utils/validation.js", "hash_validation")
    db.set_file_hash("src/models/cart.js", "hash_cart")
    db.set_file_hash("src/services/product.js", "hash_product")
    db.set_file_hash("src/services/inventory.js", "hash_inventory")
    db.set_file_hash("src/config.js", "hash_config")
    db.commit()

    return db


def make_js_file(directory: Path, rel_path: str, content: str) -> Path:
    full_path = directory / rel_path
    full_path.parent.mkdir(parents=True, exist_ok=True)
    full_path.write_text(content)
    return full_path
