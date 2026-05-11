use loom_core::{
    embedder::Embedder,
    graph::SymbolGraph,
    models::{Edge, Symbol},
    search::{compute_evolutionary, compute_semantic, compute_structural, fuse_signals},
    store::LoomDb,
    LoomConfig, Result, SearchEngine,
};
use std::collections::BTreeSet;
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
        language: "typescript".to_string(),
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
fn scoring_matches_rust_contract() {
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
            "src/session.ts",
            1,
            "function resolve_session() { return new SessionValidator(); }",
        ))
        .unwrap();
    let validator = db
        .insert_symbol(&symbol(
            "SessionValidator",
            "class",
            "src/session.ts",
            9,
            "class SessionValidator {}",
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

    let response = engine.search("resolve_session", 5, None).unwrap();
    let resolver_result = response
        .exact_hits
        .iter()
        .find(|result| result.symbol.name == "resolve_session")
        .unwrap();
    assert!(resolver_result
        .coupled
        .iter()
        .any(|entry| entry.symbol.name == "SessionValidator"));
    let neighborhood = engine.neighborhood("src/session.ts", 2).unwrap();
    assert_eq!(neighborhood.anchor.unwrap().symbol.name, "resolve_session");
}

#[test]
fn search_contract_fixture_splits_exact_and_beyond_grep_deterministically() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let exact = db
        .insert_symbol(&symbol(
            "needle",
            "function",
            "src/exact.ts",
            3,
            "function needle() { return graphOnly(); }",
        ))
        .unwrap();
    let semantic = db
        .insert_symbol(&symbol(
            "semantic_helper",
            "function",
            "src/semantic.ts",
            8,
            "function semantic_helper() { return explain(); }",
        ))
        .unwrap();
    let graph_only = db
        .insert_symbol(&symbol(
            "graphOnly",
            "function",
            "src/graph.ts",
            13,
            "function graphOnly() { return true; }",
        ))
        .unwrap();
    db.insert_edge(&edge(exact, graph_only, "graphOnly", "calls"))
        .unwrap();
    db.insert_embedding(exact, &[0.0, 1.0, 0.0]).unwrap();
    db.insert_embedding(semantic, &[0.0, 1.0, 0.0]).unwrap();
    db.insert_embedding(graph_only, &[1.0, 0.0, 0.0]).unwrap();

    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let response = engine.search("needle", 10, None).unwrap();
    assert_eq!(response.contract, "loom.search.response");
    assert_eq!(response.version, 1);
    assert!(response.index_revision.starts_with("idx-"));
    assert!(response.inspect_required);
    assert_eq!(response.budget.unit, "results");
    assert_eq!(
        response
            .exact_hits
            .iter()
            .map(|hit| hit.symbol.name.as_str())
            .collect::<Vec<_>>(),
        vec!["needle"]
    );
    assert!(response.exact_hits[0]
        .reason_codes
        .contains(&"exact:name".to_string()));
    assert!(response.exact_hits[0]
        .reason_codes
        .contains(&"semantic".to_string()));
    assert_eq!(response.exact_hits[0].symbol.file, "src/exact.ts");
    assert_eq!(response.exact_hits[0].rank, 1);
    assert_eq!(response.exact_hits[0].name, "needle");
    assert_eq!(response.exact_hits[0].kind, "function");
    assert_eq!(response.exact_hits[0].anchor.file, "src/exact.ts");
    assert_eq!(response.exact_hits[0].anchor.line, 3);
    assert!(response.exact_hits[0]
        .file_handle
        .starts_with(&format!("file:{}:", response.index_revision)));
    assert!(response.exact_hits[0].summary.contains("needle"));
    assert_eq!(response.exact_hits[0].symbol.line, 3);
    assert_eq!(
        response.exact_hits[0]
            .lexical_evidence
            .as_ref()
            .unwrap()
            .field,
        "name"
    );

    let beyond_names = response
        .beyond_grep
        .iter()
        .map(|hit| hit.symbol.name.as_str())
        .collect::<BTreeSet<_>>();
    assert!(beyond_names.contains("semantic_helper"));
    assert!(beyond_names.contains("graphOnly"));
    assert!(response
        .beyond_grep
        .iter()
        .any(|hit| hit.reason_codes.contains(&"semantic".to_string())));
    assert!(response.beyond_grep.iter().any(|hit| hit
        .reason_codes
        .iter()
        .any(|reason| reason.starts_with("graph:"))));

    let mut spans = BTreeSet::new();
    for hit in response
        .exact_hits
        .iter()
        .chain(response.beyond_grep.iter())
    {
        assert!(spans.insert((&hit.symbol.file, hit.symbol.line, hit.symbol.end_line)));
    }

    let second = engine.search("needle", 10, None).unwrap();
    let first_order = response
        .exact_hits
        .iter()
        .chain(response.beyond_grep.iter())
        .map(|hit| hit.handle.as_str())
        .collect::<Vec<_>>();
    let second_order = second
        .exact_hits
        .iter()
        .chain(second.beyond_grep.iter())
        .map(|hit| hit.handle.as_str())
        .collect::<Vec<_>>();
    assert_eq!(first_order, second_order);

    let empty_lexical = engine.search("!!!", 10, None).unwrap();
    assert!(empty_lexical.exact_hits.is_empty());
    assert!(!empty_lexical.beyond_grep.is_empty());
}

#[test]
fn inspect_resolves_symbol_and_file_handles_with_budgets_and_stale_guidance() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(
        src_dir.join("exact.ts"),
        "const prelude = 1;\n\nfunction needle() {\n  const value = 41;\n  return value + 1;\n}\n\nexport { needle };\n",
    )
    .unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let exact = db
        .insert_symbol(&symbol(
            "needle",
            "function",
            "src/exact.ts",
            3,
            "function needle() { return value + 1; }",
        ))
        .unwrap();
    db.set_file_hash("src/exact.ts", "hash").unwrap();
    db.insert_embedding(exact, &[0.0, 1.0, 0.0]).unwrap();
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let response = engine.search("needle", 5, None).unwrap();
    let hit = &response.exact_hits[0];
    let inspected = engine.inspect(&hit.handle, 2, 80, 0).unwrap();
    assert_eq!(inspected.contract, "loom.inspect.response");
    assert!(!inspected.stale);
    assert!(inspected.truncated);
    assert_eq!(inspected.handle_kind, "symbol");
    assert_eq!(inspected.anchor.as_ref().unwrap().file, "src/exact.ts");
    let snippet = inspected.snippet.as_ref().unwrap();
    assert_eq!(snippet.start_line, 3);
    assert_eq!(snippet.end_line, 4);
    assert!(snippet.text.contains("function needle"));
    assert_eq!(inspected.page.next_line_offset, Some(2));

    let file_inspected = engine.inspect(&hit.file_handle, 1, 24, 3).unwrap();
    assert_eq!(file_inspected.handle_kind, "file");
    assert!(file_inspected.truncated);
    assert_eq!(file_inspected.snippet.as_ref().unwrap().start_line, 4);
    assert!(file_inspected.page.next_line_offset.is_some());

    let stale = engine
        .inspect(&format!("symbol:idx-stale:{exact}"), 2, 80, 0)
        .unwrap();
    assert!(stale.stale);
    assert!(stale.error.unwrap().contains("rerun search"));
}

#[test]
fn evidence_pack_returns_citable_bounded_proof_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(
        src_dir.join("exact.ts"),
        "function needle() {\n  return graphOnly();\n}\n",
    )
    .unwrap();
    std::fs::write(
        src_dir.join("semantic.ts"),
        "function semantic_helper() {\n  return explain();\n}\n",
    )
    .unwrap();
    std::fs::write(
        src_dir.join("graph.ts"),
        "function graphOnly() {\n  return true;\n}\n",
    )
    .unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let exact = db
        .insert_symbol(&symbol(
            "needle",
            "function",
            "src/exact.ts",
            1,
            "function needle() { return graphOnly(); }",
        ))
        .unwrap();
    let semantic = db
        .insert_symbol(&symbol(
            "semantic_helper",
            "function",
            "src/semantic.ts",
            1,
            "function semantic_helper() { return explain(); }",
        ))
        .unwrap();
    let graph_only = db
        .insert_symbol(&symbol(
            "graphOnly",
            "function",
            "src/graph.ts",
            1,
            "function graphOnly() { return true; }",
        ))
        .unwrap();
    for file in ["src/exact.ts", "src/semantic.ts", "src/graph.ts"] {
        db.set_file_hash(file, "hash").unwrap();
    }
    db.insert_edge(&edge(exact, graph_only, "graphOnly", "calls"))
        .unwrap();
    db.insert_embedding(exact, &[0.0, 1.0, 0.0]).unwrap();
    db.insert_embedding(semantic, &[0.0, 1.0, 0.0]).unwrap();
    db.insert_embedding(graph_only, &[1.0, 0.0, 0.0]).unwrap();
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let pack = engine.evidence_pack("needle", 600).unwrap();
    assert_eq!(pack.contract, "loom.evidence_pack.response");
    assert!(!pack.inspect_required);
    assert!(!pack.exact_hits.is_empty());
    assert!(!pack.beyond_grep.is_empty());
    assert!(!pack.inspected_snippets.is_empty());
    assert!(pack
        .inspected_snippets
        .iter()
        .any(|snippet| snippet.anchor.file == "src/exact.ts"));
    assert!(pack
        .coverage_checklist
        .iter()
        .any(|item| item.item == "source_snippets" && item.status == "included"));
    assert!(pack.display_text.contains("Evidence pack"));
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
            "src/session.ts",
            1,
            "class SessionValidator {}",
        ))
        .unwrap();
    let caller = db
        .insert_symbol(&symbol(
            "make_session",
            "function",
            "src/app.ts",
            1,
            "function make_session() { return new SessionValidator(); }",
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
        .results
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
            "src/a.ts",
            1,
            "function source() {}",
        ))
        .unwrap();
    let weak = db
        .insert_symbol(&symbol(
            "weak",
            "function",
            "src/b.ts",
            1,
            "function weak() {}",
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
    assert!(related.results.iter().all(|entry| entry.score >= 0.9));
}
