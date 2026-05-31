use loom_core::{
    embedder::{build_symbol_text, Embedder, HashingEmbedder},
    graph::SymbolGraph,
    models::{BehaviorFact, Edge, FileRoleCard, Symbol},
    search::{compute_evolutionary, compute_semantic, compute_structural, fuse_signals},
    store::{FileIndexReplacement, LoomDb},
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
    config.top_coupled = 1;
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
    let coupled_validator = resolver_result
        .coupled
        .iter()
        .find(|entry| entry.symbol.name == "SessionValidator")
        .unwrap();
    assert_eq!(coupled_validator.provenance[0].relationship, "calls");
    assert_eq!(coupled_validator.provenance[0].direction, "outgoing");
    assert_eq!(coupled_validator.provenance[0].source, "graph.neighbors");
    let neighborhood = engine.neighborhood("src/session.ts", 2).unwrap();
    assert_eq!(neighborhood.anchor.unwrap().symbol.name, "resolve_session");
}

#[test]
fn hashing_search_ranks_font_measurement_symbols_above_generic_lifecycle_symbols() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 64;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let embedder = Arc::new(HashingEmbedder::new(64));
    let fixtures = [
        (
            "TerminalFontMetrics.getFont",
            "src/terminalConfigurationService.ts",
            "measure font dimensions glyph width cell height monospace measurement",
        ),
        (
            "TerminalFontMetrics.measureFont",
            "src/terminalConfigurationService.ts",
            "font measurement character width line height monospace",
        ),
        (
            "TerminalLifecycle.dispose",
            "src/terminalLifecycle.ts",
            "cleanup lifecycle disposable terminal service",
        ),
        (
            "EditorService.createEditor",
            "src/editorService.ts",
            "create editor input pane workbench service",
        ),
    ];
    for (name, file, context) in fixtures {
        let id = db
            .insert_symbol(&symbol(name, "method", file, 1, context))
            .unwrap();
        let text = build_symbol_text(name, "method", context);
        let vector = embedder.embed_single(&text).unwrap();
        db.insert_embedding(id, &vector).unwrap();
    }
    let engine = SearchEngine::new(Arc::clone(&db), Arc::clone(&embedder), None, config);

    let response = engine.search("font measurement", 10, None).unwrap();
    let ranked = response
        .exact_hits
        .iter()
        .chain(response.beyond_grep.iter())
        .map(|hit| hit.name.as_str())
        .collect::<Vec<_>>();

    assert!(ranked.len() >= 2, "{ranked:?}");
    assert!(ranked[0].contains("Font"), "{ranked:?}");
    assert!(ranked[1].contains("Font"), "{ranked:?}");
    if let Some(dispose_rank) = ranked.iter().position(|name| name.contains("dispose")) {
        assert!(dispose_rank > 1, "{ranked:?}");
    }
    if let Some(editor_rank) = ranked.iter().position(|name| name.contains("createEditor")) {
        assert!(editor_rank > 1, "{ranked:?}");
    }
}

#[test]
fn search_payload_omits_debug_scores_and_caps_beyond_grep_results() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    for index in 0..20 {
        let id = db
            .insert_symbol(&symbol(
                &format!("SemanticOnly{index}"),
                "function",
                &format!("src/semantic_{index}.ts"),
                1,
                "unrelated visible text",
            ))
            .unwrap();
        db.insert_embedding(id, &[1.0, 0.0, 0.0]).unwrap();
    }
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        None,
        config,
    );

    let response = engine.search("session concept", 20, None).unwrap();
    let json = serde_json::to_string(&response).unwrap();
    let legacy_debug_floor = json.len() + (17 * 450);

    assert!(response.exact_hits.is_empty());
    assert_eq!(response.beyond_grep.len(), 3);
    assert!(!json.contains("signal_scores"));
    assert!(
        json.len() * 2 <= legacy_debug_floor,
        "payload bytes: {}",
        json.len()
    );
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
    assert_eq!(response.budget.unit, "tokens");
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

    let broad = engine.search("needle", 500, None).unwrap();
    assert!(broad.exact_hits.len() + broad.beyond_grep.len() <= 100);
    assert_eq!(broad.limit, 100);
    assert!(broad.truncated);

    let capped = engine.search("needle", 1, None).unwrap();
    assert!(capped.exact_hits.len() + capped.beyond_grep.len() <= 1);
    assert!(capped.truncated);
    assert!(capped.budget.omitted > 0);

    let empty_lexical = engine.search("!!!", 10, None).unwrap();
    assert!(empty_lexical.exact_hits.is_empty());
    assert!(!empty_lexical.beyond_grep.is_empty());
}

#[test]
fn search_uses_bounded_file_line_rescue_for_exact_source_matches() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(
        src_dir.join("installer.ts"),
        "export function verifyInstall() {\n  const marker = 'checksum verify tarball signature';\n  return marker.length > 0;\n}\n",
    )
    .unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let verifier = db
        .insert_symbol(&symbol(
            "verifyInstall",
            "function",
            "src/installer.ts",
            1,
            "export function verifyInstall() { return marker.length > 0; }",
        ))
        .unwrap();
    db.set_file_hash("src/installer.ts", "hash").unwrap();
    db.insert_embedding(verifier, &[0.0, 1.0, 0.0]).unwrap();
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let response = engine
        .search("checksum verify tarball signature", 5, None)
        .unwrap();
    let hit = response
        .exact_hits
        .iter()
        .find(|hit| hit.symbol.name == "verifyInstall")
        .unwrap();
    assert!(hit.reason_codes.contains(&"exact:file_line".to_string()));
    assert!(hit.signal_scores.lexical > 0.0);
    let evidence = hit.lexical_evidence.as_ref().unwrap();
    assert_eq!(evidence.field, "file_line");
    assert_eq!(evidence.match_kind, "exact_phrase");
    assert!(evidence.snippet.contains("checksum verify"));
}

#[test]
fn search_file_line_rescue_scans_past_the_old_indexed_file_cap() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());

    let expected_match_files = (560..575)
        .map(|index| format!("src/file_{index:03}.ts"))
        .collect::<BTreeSet<_>>();
    for index in 0..620 {
        let file = format!("src/file_{index:03}.ts");
        let body = if expected_match_files.contains(&file) {
            "export function carrier() {\n  return targetMethod('deep match');\n}\n"
        } else {
            "export function carrier() {\n  return 'ordinary file';\n}\n"
        };
        std::fs::write(src_dir.join(format!("file_{index:03}.ts")), body).unwrap();
        let symbol_id = db
            .insert_symbol(&symbol(
                &format!("carrier_{index:03}"),
                "function",
                &file,
                1,
                "export function carrier() { return value; }",
            ))
            .unwrap();
        db.set_file_hash(&file, "hash").unwrap();
        db.insert_embedding(symbol_id, &[0.0, 1.0, 0.0]).unwrap();
    }
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let response = engine
        .search_with_budget("targetMethod", 20, None, 8_000)
        .unwrap();
    let matched_files = response
        .exact_hits
        .iter()
        .filter(|hit| hit.reason_codes.contains(&"exact:file_line".to_string()))
        .map(|hit| hit.symbol.file.clone())
        .collect::<BTreeSet<_>>();

    assert_eq!(matched_files, expected_match_files);
    assert_eq!(matched_files.len(), 15);
}

#[test]
fn search_file_line_rescue_scans_200_file_fixture_under_100ms() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let expected_match_files = (40..55)
        .map(|index| format!("src/proxy_{index:03}.ts"))
        .collect::<BTreeSet<_>>();

    for index in 0..200 {
        let file = format!("src/proxy_{index:03}.ts");
        let body = if expected_match_files.contains(&file) {
            "export function proxyCarrier() {\n  return targetMethod('proxy match');\n}\n"
        } else {
            "export function proxyCarrier() {\n  return 'ordinary file';\n}\n"
        };
        std::fs::write(src_dir.join(format!("proxy_{index:03}.ts")), body).unwrap();
        let symbol_id = db
            .insert_symbol(&symbol(
                &format!("proxyCarrier_{index:03}"),
                "function",
                &file,
                1,
                "export function proxyCarrier() { return value; }",
            ))
            .unwrap();
        db.set_file_hash(&file, "hash").unwrap();
        db.insert_embedding(symbol_id, &[0.0, 1.0, 0.0]).unwrap();
    }
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let started = std::time::Instant::now();
    let response = engine
        .search_with_budget("targetMethod", 20, None, 8_000)
        .unwrap();
    let elapsed = started.elapsed();
    let matched_files = response
        .exact_hits
        .iter()
        .filter(|hit| hit.reason_codes.contains(&"exact:file_line".to_string()))
        .map(|hit| hit.symbol.file.clone())
        .collect::<BTreeSet<_>>();

    assert_eq!(matched_files, expected_match_files);
    assert!(
        elapsed < std::time::Duration::from_millis(100),
        "search took {elapsed:?}"
    );
}

#[test]
fn search_annotates_lexical_hits_with_graph_roles() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(
        src_dir.join("a.ts"),
        "export function foo() {\n  return B.bar();\n}\n",
    )
    .unwrap();
    std::fs::write(
        src_dir.join("b.ts"),
        "export namespace B {\n  export function bar() {\n    return 1;\n  }\n}\n",
    )
    .unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let mut caller = symbol(
        "A.foo",
        "function",
        "src/a.ts",
        1,
        "export function foo() { return B.bar(); }",
    );
    caller.end_line = 3;
    let caller_id = db.insert_symbol(&caller).unwrap();
    let mut target = symbol(
        "B.bar",
        "function",
        "src/b.ts",
        2,
        "export function bar() { return 1; }",
    );
    target.end_line = 4;
    let target_id = db.insert_symbol(&target).unwrap();
    db.set_file_hash("src/a.ts", "hash").unwrap();
    db.set_file_hash("src/b.ts", "hash").unwrap();
    db.insert_edge(&edge(caller_id, target_id, "B.bar", "calls"))
        .unwrap();
    db.insert_embedding(caller_id, &[0.0, 1.0, 0.0]).unwrap();
    db.insert_embedding(target_id, &[0.0, 1.0, 0.0]).unwrap();
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let response = engine.search("bar", 10, None).unwrap();
    let caller_hit = response
        .exact_hits
        .iter()
        .find(|hit| hit.name == "A.foo")
        .unwrap();
    assert_eq!(caller_hit.graph_role.as_deref(), Some("caller"));
    assert!(caller_hit
        .reason_codes
        .contains(&"graph_role:caller".to_string()));

    let target_hit = response
        .exact_hits
        .iter()
        .find(|hit| hit.name == "B.bar")
        .unwrap();
    assert_eq!(target_hit.graph_role.as_deref(), Some("definition"));
}

#[test]
fn search_response_stays_inside_default_token_budget() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());

    for index in 0..60 {
        let file = format!("src/budget_{index:03}.ts");
        std::fs::write(
            src_dir.join(format!("budget_{index:03}.ts")),
            format!(
                "export function budgetCarrier{index}() {{\n  return 'budgetNeedle marker {index}';\n}}\n"
            ),
        )
        .unwrap();
        let symbol_id = db
            .insert_symbol(&symbol(
                &format!("budgetCarrier{index}"),
                "function",
                &file,
                1,
                "export function budgetCarrier() { return marker; }",
            ))
            .unwrap();
        db.set_file_hash(&file, "hash").unwrap();
        db.insert_embedding(symbol_id, &[0.0, 1.0, 0.0]).unwrap();
    }
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let response = engine.search("budgetNeedle", 100, None).unwrap();
    let json = serde_json::to_string(&response).unwrap();

    assert!(json.len() <= 8_000, "payload bytes: {}", json.len());
    assert_eq!(response.budget.unit, "tokens");
    assert!(response.truncated);
    assert!(response.budget.returned <= 2_000);
}

#[test]
fn search_returns_file_line_hits_with_zero_index() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(
        src_dir.join("loose.ts"),
        "export function someFunction() {\n  return 42;\n}\n",
    )
    .unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let response = engine.search("someFunction", 5, None).unwrap();
    let hit = response
        .exact_hits
        .iter()
        .find(|hit| hit.anchor.file == "src/loose.ts")
        .unwrap_or_else(|| panic!("missing zero-index hit: {response:#?}"));
    assert_eq!(hit.kind, "file_match");
    assert!(hit.handle.starts_with("file:"));
    assert!(hit.reason_codes.contains(&"exact:file_line".to_string()));
    assert!(hit
        .lexical_evidence
        .as_ref()
        .unwrap()
        .snippet
        .contains("someFunction"));

    let inspected = engine.inspect(&hit.handle, 2, 120, 0).unwrap();
    assert_eq!(inspected.handle_kind, "file");
    assert!(inspected
        .snippet
        .as_ref()
        .unwrap()
        .text
        .contains("someFunction"));
}

#[test]
fn search_marks_response_building_when_index_is_not_ready() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("loose.ts"), "const lazyNeedle = 1;\n").unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    )
    .with_index_ready(false);

    let response = engine.search("lazyNeedle", 5, None).unwrap();

    assert_eq!(response.index_status.as_deref(), Some("building"));
    assert!(response
        .exact_hits
        .iter()
        .any(|hit| hit.anchor.file == "src/loose.ts"));
}

#[test]
fn search_modes_return_transitive_callers_and_impact_in_one_call() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let a = db
        .insert_symbol(&symbol("A", "function", "src/a.ts", 1, "A calls B"))
        .unwrap();
    let b = db
        .insert_symbol(&symbol("B", "function", "src/b.ts", 1, "B calls C"))
        .unwrap();
    let c = db
        .insert_symbol(&symbol("C", "function", "src/c.ts", 1, "C calls D"))
        .unwrap();
    let d = db
        .insert_symbol(&symbol("D", "function", "src/d.ts", 1, "D calls E"))
        .unwrap();
    let e = db
        .insert_symbol(&symbol("E", "function", "src/e.ts", 1, "E terminal"))
        .unwrap();
    db.insert_edge(&edge(a, b, "B", "calls")).unwrap();
    db.insert_edge(&edge(b, c, "C", "calls")).unwrap();
    db.insert_edge(&edge(c, d, "D", "calls")).unwrap();
    db.insert_edge(&edge(d, e, "E", "calls")).unwrap();
    for id in [a, b, c, d, e] {
        db.insert_embedding(id, &[0.0, 1.0, 0.0]).unwrap();
    }
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let callers = engine
        .search_mode_with_budget("C", 10, None, Some("callers"), 8_000)
        .unwrap();
    let caller_names = callers
        .exact_hits
        .iter()
        .map(|hit| hit.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(caller_names, vec!["B", "A"]);
    assert!(callers
        .exact_hits
        .iter()
        .all(|hit| hit.graph_role.as_deref() == Some("caller")));

    let impact = engine
        .search_mode_with_budget("C", 10, None, Some("impact"), 8_000)
        .unwrap();
    let impact_names = impact
        .exact_hits
        .iter()
        .map(|hit| hit.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(impact_names, vec!["D", "E"]);
    assert!(impact
        .exact_hits
        .iter()
        .all(|hit| hit.reason_codes.contains(&"mode:impact".to_string())));
}

#[test]
fn search_file_line_rescue_does_not_anchor_top_level_text_to_nearest_symbol() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(
        src_dir.join("module.ts"),
        "import 'top level marker phrase';\n\nexport function unrelated() {\n  return 1;\n}\n",
    )
    .unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let symbol_id = db
        .insert_symbol(&symbol(
            "unrelated",
            "function",
            "src/module.ts",
            3,
            "export function unrelated() { return 1; }",
        ))
        .unwrap();
    db.set_file_hash("src/module.ts", "hash").unwrap();
    db.insert_embedding(symbol_id, &[0.0, 1.0, 0.0]).unwrap();
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let response = engine.search("top level marker phrase", 5, None).unwrap();
    assert!(!response
        .exact_hits
        .iter()
        .any(|hit| hit.reason_codes.contains(&"exact:file_line".to_string())));
}

#[test]
fn search_file_line_rescue_anchors_to_smallest_containing_symbol() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(
        src_dir.join("command.ts"),
        "export class CacheCommand {\n  async execute() {\n    return 'needle execute marker';\n  }\n}\n",
    )
    .unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let mut class_symbol = symbol(
        "CacheCommand",
        "class",
        "src/command.ts",
        1,
        "export class CacheCommand {}",
    );
    class_symbol.end_line = 5;
    let class_id = db.insert_symbol(&class_symbol).unwrap();
    let mut method_symbol = symbol(
        "CacheCommand.execute",
        "method",
        "src/command.ts",
        2,
        "async execute() { return 'needle execute marker'; }",
    );
    method_symbol.end_line = 4;
    let method_id = db.insert_symbol(&method_symbol).unwrap();
    db.set_file_hash("src/command.ts", "hash").unwrap();
    db.insert_embedding(class_id, &[0.0, 1.0, 0.0]).unwrap();
    db.insert_embedding(method_id, &[0.0, 1.0, 0.0]).unwrap();
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let response = engine.search("needle execute marker", 10, None).unwrap();
    assert!(response
        .exact_hits
        .iter()
        .any(|hit| hit.name == "CacheCommand.execute"
            && hit.reason_codes.contains(&"exact:file_line".to_string())));
    assert!(!response
        .exact_hits
        .iter()
        .any(|hit| hit.name == "CacheCommand"
            && hit.reason_codes.contains(&"exact:file_line".to_string())));
}

#[test]
fn symbols_enumerates_method_suffixes_with_file_prefix_and_kind() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    for (name, kind, file, line) in [
        (
            "UseCommand.execute",
            "method",
            "sources/commands/Use.ts",
            25,
        ),
        ("UpCommand.execute", "method", "sources/commands/Up.ts", 33),
        (
            "InstallLocalCommand.execute",
            "method",
            "sources/commands/InstallLocal.ts",
            21,
        ),
        (
            "InstallLocalCommand.executeFlag",
            "variable",
            "sources/commands/InstallLocal.ts",
            22,
        ),
        ("Engine.execute", "method", "sources/Engine.ts", 99),
        ("executeHelper", "function", "sources/commands/helper.ts", 4),
        (
            "CommandService",
            "class",
            "src/workbench/services/commands/common/commandService.ts",
            15,
        ),
        (
            "StandaloneCommandService",
            "class",
            "src/editor/standalone/browser/standaloneServices.ts",
            371,
        ),
    ] {
        let context = if name == "CommandService" {
            "export class CommandService extends Disposable implements ICommandService"
        } else if name == "StandaloneCommandService" {
            "export class StandaloneCommandService implements ICommandService"
        } else {
            "async execute() {}"
        };
        db.insert_symbol(&symbol(name, kind, file, line, context))
            .unwrap();
    }
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let response = engine
        .symbols("execute", Some("sources/commands"), Some("method"), 10)
        .unwrap();
    assert_eq!(response.contract, "loom.symbols.response");
    assert!(!response.truncated);
    let names = response
        .results
        .iter()
        .map(|hit| hit.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "InstallLocalCommand.execute",
            "UpCommand.execute",
            "UseCommand.execute",
        ]
    );
    assert!(response.results.iter().all(|hit| hit
        .reason_codes
        .contains(&"symbol:method_suffix".to_string())));
    assert!(response
        .results
        .iter()
        .all(|hit| hit.anchor.file.starts_with("sources/commands")));

    let relaxed = engine
        .symbols(
            "InstallLocalCommand.execute",
            Some("sources/commands"),
            Some("function"),
            5,
        )
        .unwrap();
    assert_eq!(relaxed.results.len(), 1);
    assert_eq!(relaxed.results[0].name, "InstallLocalCommand.execute");
    assert_eq!(relaxed.results[0].kind, "method");
    assert!(relaxed.results[0]
        .reason_codes
        .contains(&"kind:relaxed-function-method".to_string()));

    let search = engine
        .search("InstallLocalCommand.execute", 5, Some("function"))
        .unwrap();
    assert_eq!(search.exact_hits.len(), 1);
    assert_eq!(search.exact_hits[0].name, "InstallLocalCommand.execute");
    assert_eq!(search.exact_hits[0].kind, "method");
    assert!(search.exact_hits[0]
        .reason_codes
        .contains(&"symbol:exact".to_string()));

    let token_search = engine
        .search("install local command execute", 5, None)
        .unwrap();
    assert!(token_search
        .exact_hits
        .iter()
        .any(|hit| hit.name == "InstallLocalCommand.execute"
            && hit
                .reason_codes
                .contains(&"symbol:ordered_tokens".to_string())));

    let camel_search = engine
        .search("InstallLocalCommandExecute", 5, None)
        .unwrap();
    assert_eq!(camel_search.query_intent.intent, "symbol");
    assert!(camel_search
        .exact_hits
        .iter()
        .any(|hit| hit.name == "InstallLocalCommand.execute"
            && hit
                .reason_codes
                .contains(&"symbol:ordered_tokens".to_string())));

    let exact_symbol_search = engine.search("CommandService", 5, None).unwrap();
    assert_eq!(exact_symbol_search.exact_hits[0].name, "CommandService");
    assert!(exact_symbol_search.exact_hits[0]
        .reason_codes
        .contains(&"symbol:exact".to_string()));

    let mixed_query_search = engine.search("class CommandService", 5, None).unwrap();
    assert_eq!(mixed_query_search.exact_hits[0].name, "CommandService");
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
    assert!(stale.snippet.is_some());
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
    std::fs::write(
        src_dir.join("fact.ts"),
        "function strictMode() {\n  return process.env.COREPACK_ENABLE_STRICT;\n}\n",
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
    let fact_symbols = vec![symbol(
        "strictMode",
        "function",
        "src/fact.ts",
        1,
        "function strictMode() { return process.env.COREPACK_ENABLE_STRICT; }",
    )];
    let fact_embeddings = vec![vec![0.0, 1.0, 0.0]];
    let facts = vec![BehaviorFact {
        id: None,
        fact_type: "env_var".to_string(),
        value: "COREPACK_ENABLE_STRICT".to_string(),
        file: "src/fact.ts".to_string(),
        line: 2,
        end_line: 2,
        enclosing_symbol_id: None,
        enclosing_symbol_name: Some("strictMode".to_string()),
        occurrence_count: 1,
    }];
    let role_card = FileRoleCard {
        file: "src/fact.ts".to_string(),
        content_hash: "hash-fact".to_string(),
        primary_responsibility: "strict mode env lookup".to_string(),
        exported_symbols: vec!["strictMode".to_string()],
        imported_dependencies: Vec::new(),
        behavior_facts: vec!["env_var:COREPACK_ENABLE_STRICT".to_string()],
        centrality: 0.0,
        tests_touching: Vec::new(),
        top_related_files: Vec::new(),
    };
    db.replace_file_index(
        FileIndexReplacement {
            path: "src/fact.ts",
            content_hash: "hash-fact",
            symbols: &fact_symbols,
            embeddings: &fact_embeddings,
            embedding_fingerprint: "test-fingerprint",
            behavior_facts: &facts,
            callsites: &[],
            aliases: &[],
            role_card: &role_card,
        },
        |_| Ok(Vec::new()),
    )
    .unwrap();
    db.resolve_signal_enclosures().unwrap();
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
    assert!(!pack.inspected_snippets.is_empty());
    assert!(pack
        .inspected_snippets
        .iter()
        .any(|snippet| snippet.anchor.file == "src/exact.ts"));
    assert!(pack
        .coverage_checklist
        .iter()
        .any(|item| item.item == "source_snippets" && item.status == "included"));
    let env_pack = engine.evidence_pack("COREPACK_ENABLE_STRICT", 600).unwrap();
    let fact_hit = env_pack
        .behavior_facts
        .iter()
        .find(|hit| hit.fact.value == "COREPACK_ENABLE_STRICT")
        .unwrap();
    assert!(fact_hit.handle.starts_with("fact:"));
    assert_eq!(fact_hit.name, "COREPACK_ENABLE_STRICT");
    assert_eq!(fact_hit.kind, "env_var");
    assert!(fact_hit.summary.contains("COREPACK_ENABLE_STRICT"));
    assert!(fact_hit
        .reason_codes
        .iter()
        .any(|reason| reason == "fact:env_var"));
    assert_eq!(fact_hit.anchor.file, "src/fact.ts");
    let inspected_fact = engine.inspect(&fact_hit.handle, 1, 120, 0).unwrap();
    assert_eq!(inspected_fact.handle_kind, "fact");
    assert!(inspected_fact
        .snippet
        .as_ref()
        .unwrap()
        .text
        .contains("COREPACK_ENABLE_STRICT"));
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
    let caller = impact
        .results
        .iter()
        .find(|entry| entry.symbol.name == "make_session")
        .unwrap();
    assert_eq!(caller.provenance[0].source, "unresolved_name_edge");
    assert_eq!(caller.provenance[0].relationship, "calls");
    assert_eq!(
        db.get_symbol_by_id(target).unwrap().unwrap().name,
        "SessionValidator"
    );
}

#[test]
fn impact_kind_selects_target_without_filtering_callers() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let target = db
        .insert_symbol(&symbol(
            "runVersion",
            "function",
            "src/corepackUtils.ts",
            10,
            "export async function runVersion() {}",
        ))
        .unwrap();
    let method_caller = db
        .insert_symbol(&symbol(
            "Engine.executePackageManagerRequest",
            "method",
            "src/Engine.ts",
            30,
            "async executePackageManagerRequest() { return corepackUtils.runVersion(); }",
        ))
        .unwrap();
    db.insert_edge(&edge(method_caller, target, "runVersion", "calls"))
        .unwrap();
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let impact = engine
        .impact("runVersion", Some("src/corepackUtils.ts"), Some("function"))
        .unwrap();
    assert!(impact
        .results
        .iter()
        .any(|entry| entry.symbol.name == "Engine.executePackageManagerRequest"));
}

#[test]
fn related_kind_selects_target_without_filtering_neighbors() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(temp.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    config.coupling_threshold = 0.0;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let target = db
        .insert_symbol(&symbol(
            "targetFn",
            "function",
            "src/a.ts",
            1,
            "function targetFn() {}",
        ))
        .unwrap();
    let method_neighbor = db
        .insert_symbol(&symbol(
            "Widget.execute",
            "method",
            "src/widget.ts",
            10,
            "async execute() { return targetFn(); }",
        ))
        .unwrap();
    db.insert_edge(&edge(method_neighbor, target, "targetFn", "calls"))
        .unwrap();
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(Arc::new(SymbolGraph::build_from_db(&db).unwrap())),
        config,
    );

    let related = engine
        .related("targetFn", Some("src/a.ts"), Some("function"))
        .unwrap();
    assert!(related
        .results
        .iter()
        .any(|entry| entry.symbol.name == "Widget.execute"));
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
