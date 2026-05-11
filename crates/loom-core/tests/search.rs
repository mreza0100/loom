use loom_core::{
    embedder::Embedder,
    graph::SymbolGraph,
    models::{Edge, Symbol},
    search::{compute_evolutionary, compute_semantic, compute_structural, fuse_signals},
    store::LoomDb,
    LoomConfig, Result, SearchEngine,
};
use std::sync::Arc;

#[derive(Debug)]
struct MockEmbedder {
    dimensions: usize,
}

impl Embedder for MockEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| {
                let mut vector = vec![0.0; self.dimensions];
                if text.contains("session") || text.contains("Session") {
                    vector[0] = 1.0;
                } else {
                    vector[1] = 1.0;
                }
                vector
            })
            .collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

fn symbol(name: &str, kind: &str, file: &str, line: i64, context: &str) -> Symbol {
    Symbol {
        id: None,
        name: name.to_string(),
        kind: kind.to_string(),
        file: file.to_string(),
        line,
        end_line: line + 4,
        language: "python".to_string(),
        context: context.to_string(),
    }
}

fn edge(source_id: i64, target_id: i64, target_name: &str, relationship: &str) -> Edge {
    Edge {
        id: None,
        source_id,
        target_id: Some(target_id),
        target_name: target_name.to_string(),
        target_file: None,
        relationship: relationship.to_string(),
        confidence: 0.9,
        original_name: None,
    }
}

fn unresolved_edge(source_id: i64, target_name: &str, relationship: &str) -> Edge {
    Edge {
        id: None,
        source_id,
        target_id: None,
        target_name: target_name.to_string(),
        target_file: None,
        relationship: relationship.to_string(),
        confidence: 0.0,
        original_name: None,
    }
}

#[test]
fn scoring_matches_python_contract() {
    let config = LoomConfig::default_for_target(".");
    assert_eq!(compute_structural("calls", 0.8, 2), 0.4);
    assert_eq!(compute_semantic(0.25), 0.75);
    assert_eq!(compute_evolutionary(5, 0.5, 10), 0.5);
    let fused = fuse_signals(0.8, 0.4, 0.0, &config);
    assert!(fused.combined > 0.5);
    assert!(fused.breakdown().contains("structural="));
}

#[test]
fn evolutionary_scoring_uses_recency_as_tie_breaker() {
    let stale = compute_evolutionary(5, 0.0, 10);
    let fresh = compute_evolutionary(5, 1.0, 10);

    assert!(fresh > stale);
    assert_eq!(compute_evolutionary(0, 0.0, 10), 0.0);
    assert_eq!(compute_evolutionary(20, 2.0, 10), 1.0);
}

#[test]
fn search_returns_hybrid_results_with_coupled_symbols() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let resolver = db
        .insert_symbol(&symbol(
            "resolve_session",
            "function",
            "src/session.py",
            1,
            "def resolve_session(): return SessionValidator()",
        ))
        .unwrap();
    let validator = db
        .insert_symbol(&symbol(
            "SessionValidator",
            "class",
            "src/session.py",
            9,
            "class SessionValidator: pass",
        ))
        .unwrap();
    db.insert_edge(&edge(resolver, validator, "SessionValidator", "calls"))
        .unwrap();
    db.insert_embedding(resolver, &[1.0, 0.0, 0.0]).unwrap();
    db.insert_embedding(validator, &[1.0, 0.0, 0.0]).unwrap();
    let graph = Arc::new(SymbolGraph::build_from_db(&db).unwrap());
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(graph),
        config,
    );

    let results = engine.search("session resolver", 5, None).unwrap();
    let resolver_result = results
        .iter()
        .find(|result| result.symbol.name == "resolve_session")
        .unwrap();
    assert!(resolver_result
        .coupled
        .iter()
        .any(|entry| entry.symbol.name == "SessionValidator"));
    let neighborhood = engine.neighborhood("src/session.py", 2).unwrap();
    assert_eq!(neighborhood.anchor.unwrap().name, "resolve_session");
}

#[test]
fn impact_includes_unresolved_name_callers() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let target = db
        .insert_symbol(&symbol(
            "SessionValidator",
            "class",
            "src/session.py",
            1,
            "class SessionValidator: pass",
        ))
        .unwrap();
    let caller = db
        .insert_symbol(&symbol(
            "make_session",
            "function",
            "src/app.py",
            1,
            "def make_session(): return SessionValidator()",
        ))
        .unwrap();
    db.insert_edge(&unresolved_edge(caller, "SessionValidator", "calls"))
        .unwrap();
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let impact = engine.impact("SessionValidator", None, None).unwrap();
    assert!(impact
        .iter()
        .any(|entry| entry.symbol.name == "make_session"));
    assert_eq!(
        db.get_symbol_by_id(target).unwrap().unwrap().name,
        "SessionValidator"
    );
}

#[test]
fn related_respects_coupling_threshold() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    config.coupling_threshold = 0.9;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let source = db
        .insert_symbol(&symbol(
            "source",
            "function",
            "src/a.py",
            1,
            "def source(): pass",
        ))
        .unwrap();
    let weak = db
        .insert_symbol(&symbol(
            "weak",
            "function",
            "src/b.py",
            1,
            "def weak(): pass",
        ))
        .unwrap();
    db.insert_edge(&edge(source, weak, "weak", "imports"))
        .unwrap();
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let related = engine.related("source", None, None).unwrap();
    assert!(related.iter().all(|entry| entry.score >= 0.9));
}
