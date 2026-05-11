use loom_core::config::LoomConfig;
use loom_core::graph::SymbolGraph;
use loom_core::models::{
    CoupledSymbol, CouplingScore, Edge, FileState, ParsedEdge, SearchResult, Symbol,
};
use loom_core::store::{sanitize_fts_query, LoomDb, ReaderPragma};
use loom_core::LoomError;
use std::fs;
use tempfile::TempDir;

fn symbol(name: &str, file: &str, line: i64) -> Symbol {
    Symbol {
        id: None,
        name: name.to_string(),
        kind: "function".to_string(),
        file: file.to_string(),
        line,
        end_line: line + 3,
        language: "python".to_string(),
        context: format!("def {name}(): ..."),
    }
}

fn edge(source_id: i64, target_id: Option<i64>, target_name: &str, confidence: f64) -> Edge {
    Edge {
        id: None,
        source_id,
        target_id,
        target_name: target_name.to_string(),
        target_file: None,
        relationship: "calls".to_string(),
        confidence,
        original_name: None,
    }
}

fn temp_config() -> (TempDir, LoomConfig) {
    let temp = tempfile::tempdir().unwrap();
    let config = LoomConfig::default_for_target(temp.path());
    (temp, config)
}

fn temp_db() -> (TempDir, LoomDb) {
    let (temp, config) = temp_config();
    let db = LoomDb::open(config).unwrap();
    (temp, db)
}

#[test]
fn config_defaults_and_missing_toml_fallback() {
    let (temp, config) = temp_config();
    let loaded = LoomConfig::load(temp.path()).unwrap();
    assert_eq!(loaded.db_path, config.db_path);
    assert_eq!(
        loaded.embedding_model,
        "jinaai/jina-embeddings-v2-base-code"
    );
    assert_eq!(loaded.embedding_dimensions, 768);
    assert_eq!(loaded.structural_weight, 0.45);
    assert!(loaded.excluded_dirs.contains(".git"));
    assert!(loaded.watch_extensions.contains(".rs"));
}

#[test]
fn config_partial_override_and_db_path_creation() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".loom")).unwrap();
    fs::write(
        temp.path().join(".loom/config.toml"),
        r#"
db_path = ".loom/custom.db"
embedding_dimensions = 4
excluded_dirs = ["node_modules"]
semantic_weight = 0.5
"#,
    )
    .unwrap();

    let loaded = LoomConfig::load(temp.path()).unwrap();
    assert_eq!(loaded.embedding_dimensions, 4);
    assert_eq!(loaded.semantic_weight, 0.5);
    assert!(loaded.excluded_dirs.contains("node_modules"));
    assert!(loaded.excluded_dirs.contains(".loom"));
    let db_path = loaded.resolve_db_path().unwrap();
    assert_eq!(db_path, temp.path().join(".loom/custom.db"));
    assert!(db_path.parent().unwrap().exists());
}

#[test]
fn config_invalid_toml_and_invalid_weights() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".loom")).unwrap();
    fs::write(
        temp.path().join(".loom/config.toml"),
        "structural_weight = [",
    )
    .unwrap();
    assert!(matches!(
        LoomConfig::load(temp.path()),
        Err(LoomError::ConfigParse { .. })
    ));

    fs::write(
        temp.path().join(".loom/config.toml"),
        "structural_weight = -0.1",
    )
    .unwrap();
    assert!(matches!(
        LoomConfig::load(temp.path()),
        Err(LoomError::InvalidConfig(_))
    ));
}

#[test]
fn model_serde_round_trips() {
    let sym = symbol("resolve_session", "src/session.py", 7);
    let parsed = ParsedEdge {
        source_name: "resolve_session".to_string(),
        target_name: "SessionValidator".to_string(),
        relationship: "calls".to_string(),
        target_file: Some("src/session.py".to_string()),
    };
    let edge = edge(1, Some(2), "SessionValidator", 0.9);
    let file_state = FileState {
        path: "src/session.py".to_string(),
        content_hash: "abc".to_string(),
        last_indexed: "2026-05-11T00:00:00Z".to_string(),
    };
    let score = CouplingScore {
        structural: 0.8,
        semantic: 0.6,
        evolutionary: 0.2,
        combined: 0.62,
    };
    assert_eq!(
        score.breakdown(),
        "structural=0.80 + semantic=0.60 + evolutionary=0.20"
    );
    let coupled = CoupledSymbol {
        symbol: sym.clone(),
        score: 0.7,
        reason: "structural".to_string(),
    };
    let result = SearchResult {
        symbol: sym.clone(),
        score: 0.9,
        coupled: vec![coupled],
    };

    assert_eq!(
        serde_json::from_str::<Symbol>(&serde_json::to_string(&sym).unwrap()).unwrap(),
        sym
    );
    assert_eq!(
        serde_json::from_str::<ParsedEdge>(&serde_json::to_string(&parsed).unwrap()).unwrap(),
        parsed
    );
    assert_eq!(
        serde_json::from_str::<Edge>(&serde_json::to_string(&edge).unwrap()).unwrap(),
        edge
    );
    assert_eq!(
        serde_json::from_str::<FileState>(&serde_json::to_string(&file_state).unwrap()).unwrap(),
        file_state
    );
    assert_eq!(
        serde_json::from_str::<SearchResult>(&serde_json::to_string(&result).unwrap()).unwrap(),
        result
    );
}

#[test]
fn schema_pragmas_and_symbol_edge_crud() {
    let (_temp, db) = temp_db();
    assert_eq!(
        db.reader_pragma_value(ReaderPragma::ForeignKeys).unwrap(),
        "1"
    );
    assert_eq!(
        db.reader_pragma_value(ReaderPragma::JournalMode)
            .unwrap()
            .to_lowercase(),
        "wal"
    );

    let source_id = db.insert_symbol(&symbol("caller", "src/a.py", 1)).unwrap();
    let target_id = db.insert_symbol(&symbol("target", "src/b.py", 2)).unwrap();
    let edge_id = db
        .insert_edge(&edge(source_id, Some(target_id), "target", 0.91))
        .unwrap();

    let found = db.get_symbol_by_id(source_id).unwrap().unwrap();
    assert_eq!(found.name, "caller");
    let outgoing = db.get_edges_from(source_id).unwrap();
    assert_eq!(outgoing[0].id, Some(edge_id));
    assert_eq!(outgoing[0].confidence, 0.91);
    assert_eq!(db.get_edges_to(target_id).unwrap().len(), 1);
}

#[test]
fn unresolved_edge_resolution_and_remove_edges_for_source() {
    let (_temp, db) = temp_db();
    let source_id = db.insert_symbol(&symbol("caller", "src/a.py", 1)).unwrap();
    let target_id = db.insert_symbol(&symbol("target", "src/b.py", 2)).unwrap();
    let edge_id = db
        .insert_edge(&edge(source_id, None, "target", 0.0))
        .unwrap();
    assert_eq!(db.get_unresolved_edges().unwrap().len(), 1);

    db.resolve_edge(edge_id, target_id, 0.88).unwrap();
    assert!(db.get_unresolved_edges().unwrap().is_empty());
    assert_eq!(
        db.get_edges_to_by_name("target").unwrap()[0].target_id,
        Some(target_id)
    );

    db.remove_edges_for_source(source_id).unwrap();
    assert!(db.get_edges_from(source_id).unwrap().is_empty());
}

#[test]
fn remove_file_cascades_and_nullifies_related_data() {
    let (_temp, db) = temp_db();
    let doomed = db
        .insert_symbol(&symbol("doomed", "src/dead.py", 1))
        .unwrap();
    let caller = db
        .insert_symbol(&symbol("caller", "src/live.py", 2))
        .unwrap();
    db.insert_edge(&edge(doomed, Some(caller), "caller", 0.7))
        .unwrap();
    db.insert_edge(&edge(caller, Some(doomed), "doomed", 0.8))
        .unwrap();
    db.insert_embedding(doomed, &[0.0; 768]).unwrap();
    db.set_file_hash("src/dead.py", "hash").unwrap();

    db.remove_file("src/dead.py").unwrap();

    assert!(db.get_symbol_by_id(doomed).unwrap().is_none());
    assert!(db.get_edges_from(doomed).unwrap().is_empty());
    let incoming = db.get_edges_from(caller).unwrap();
    assert_eq!(incoming[0].target_id, None);
    assert_eq!(incoming[0].confidence, 0.0);
    assert_eq!(db.get_file_hash("src/dead.py").unwrap(), None);
    assert_eq!(db.get_stats().unwrap().vectors, 0);
}

#[test]
fn fuzzy_lookup_strategies() {
    let (_temp, db) = temp_db();
    db.insert_symbol(&symbol("Foo.bar", "src/pkg/foo.py", 1))
        .unwrap();
    db.insert_symbol(&symbol("_hidden", "src/pkg/hidden.py", 1))
        .unwrap();
    db.insert_symbol(&symbol("named", "lib/feature.py", 1))
        .unwrap();

    assert_eq!(
        db.get_symbol_by_name_fuzzy("named", Some("feature.py"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        db.get_symbol_by_name_fuzzy("bar", None).unwrap()[0].name,
        "Foo.bar"
    );
    assert_eq!(
        db.get_symbol_by_name_fuzzy("hidden", None).unwrap()[0].name,
        "_hidden"
    );
    assert!(db
        .get_symbol_by_name_fuzzy("missing", None)
        .unwrap()
        .is_empty());
}

#[test]
fn fts_sanitization_and_search() {
    let (_temp, db) = temp_db();
    db.insert_symbol(&symbol("Session.validate", "src/session.py", 1))
        .unwrap();
    db.insert_symbol(&symbol("logical_gate", "src/logic.py", 2))
        .unwrap();

    assert_eq!(
        sanitize_fts_query("AND Session.validate"),
        "\"AND\" \"Session.validate\""
    );
    assert!(db.search_fts("", 10).unwrap().is_empty());
    assert_eq!(
        db.search_fts("Session.validate", 10).unwrap()[0].name,
        "Session.validate"
    );
    assert_eq!(db.search_fts("AND", 10).unwrap().len(), 0);
    assert!(matches!(
        db.search_fts("Session", 1_001),
        Err(LoomError::InvalidInput(_))
    ));
}

#[test]
fn vector_dimension_mismatch_and_top_k_search() {
    let (_temp, db) = temp_db();
    let first = db.insert_symbol(&symbol("first", "src/a.py", 1)).unwrap();
    let second = db.insert_symbol(&symbol("second", "src/b.py", 1)).unwrap();
    assert!(matches!(
        db.insert_embedding(first, &[1.0, 2.0]),
        Err(LoomError::VectorDimension {
            expected: 768,
            actual: 2
        })
    ));

    let base = vec![0.0; 768];
    let mut far = vec![0.0; 768];
    far[0] = 10.0;
    db.insert_embedding(first, &base).unwrap();
    db.insert_embedding(second, &far).unwrap();
    let results = db.search_vectors(&base, 2).unwrap();
    assert_eq!(results[0].0, first);
    assert_eq!(results[1].0, second);
}

#[test]
fn cochange_and_stats() {
    let (_temp, db) = temp_db();
    db.upsert_cochange("b.py", "a.py", 2).unwrap();
    db.upsert_cochange("a.py", "c.py", 4).unwrap();
    db.upsert_cochange("a.py", "b.py", 7).unwrap();

    assert_eq!(db.get_cochange_frequency("a.py", "b.py").unwrap(), 7);
    assert_eq!(db.get_cochange_frequency("missing.py", "b.py").unwrap(), 0);
    assert_eq!(
        db.get_top_cochanges("a.py", 2).unwrap()[0],
        ("b.py".to_string(), 7)
    );
    assert!(matches!(
        db.get_top_cochanges("a.py", 1_001),
        Err(LoomError::InvalidInput(_))
    ));
    assert_eq!(db.get_stats().unwrap().cochange_pairs, 2);
}

#[test]
fn graph_rebuild_traversal_decay_centrality_and_missing_nodes() {
    let (_temp, db) = temp_db();
    let a = db.insert_symbol(&symbol("a", "a.py", 1)).unwrap();
    let b = db.insert_symbol(&symbol("b", "b.py", 1)).unwrap();
    let c = db.insert_symbol(&symbol("c", "c.py", 1)).unwrap();
    db.insert_edge(&edge(a, Some(b), "b", 0.3)).unwrap();
    db.insert_edge(&edge(a, Some(b), "b", 0.9)).unwrap();
    db.insert_edge(&edge(c, Some(a), "a", 0.8)).unwrap();
    db.insert_edge(&edge(c, None, "ghost", 0.0)).unwrap();

    let graph = SymbolGraph::build_from_db(&db).unwrap();
    assert_eq!(graph.node_count(), 3);
    assert_eq!(graph.edge_count(), 2);
    assert_eq!(graph.dependencies(a, 1)[0].symbol_id, b);
    assert_eq!(graph.dependencies(a, 1)[0].confidence, 0.9);
    assert_eq!(graph.dependents(a, 1)[0].symbol_id, c);
    assert_eq!(graph.shortest_path(c, b).unwrap(), vec![c, a, b]);
    assert_eq!(graph.impact_radius(b, 2), vec![(a, 0.9), (c, 0.4)]);
    assert_eq!(graph.centrality(1)[0].0, a);
    assert!(graph.dependencies(999, 2).is_empty());
    assert!(graph.neighbors_with_metadata(999, 2).is_empty());
}
