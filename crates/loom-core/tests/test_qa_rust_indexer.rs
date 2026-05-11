use loom_core::{
    embedder::Embedder,
    indexer::{EdgeResolver, IndexPipeline},
    models::{Edge, Symbol},
    store::LoomDb,
    LoomConfig, LoomError, Result,
};
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

fn symbol(name: &str, file: &str) -> Symbol {
    Symbol {
        id: None,
        name: name.to_string(),
        kind: "function".to_string(),
        file: file.to_string(),
        line: 1,
        end_line: 1,
        language: "python".to_string(),
        context: format!("def {name}(): pass"),
    }
}

fn edge(
    source_id: i64,
    target_name: &str,
    target_file: Option<&str>,
    relationship: &str,
    original_name: Option<&str>,
) -> Edge {
    Edge {
        id: None,
        source_id,
        target_id: None,
        target_name: target_name.to_string(),
        target_file: target_file.map(str::to_string),
        relationship: relationship.to_string(),
        confidence: 0.0,
        original_name: original_name.map(str::to_string),
    }
}

fn temp_db_with_dimensions(dimensions: usize) -> (tempfile::TempDir, LoomDb) {
    let dir = tempdir().unwrap();
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = dimensions;
    config.enable_git_analysis = false;
    (dir, LoomDb::open(config).unwrap())
}

#[test]
fn qa_import_alias_member_resolution_uses_original_export_confidence() {
    let (_dir, db) = temp_db_with_dimensions(3);
    let imported_method = db
        .insert_symbol(&symbol("OriginalService.fetch", "src/service.py"))
        .unwrap();
    let caller = db.insert_symbol(&symbol("caller", "src/app.py")).unwrap();

    db.insert_edge(&edge(
        caller,
        "AliasService",
        Some("src/service.py"),
        "imports",
        Some("OriginalService"),
    ))
    .unwrap();
    db.insert_edge(&edge(caller, "AliasService.fetch", None, "calls", None))
        .unwrap();

    assert_eq!(EdgeResolver::new(&db).resolve_all().unwrap(), 1);
    let resolved = db
        .get_edges_to_by_name("AliasService.fetch")
        .unwrap()
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(resolved.target_id, Some(imported_method));
    assert_eq!(resolved.confidence, 0.95);
}

#[test]
fn qa_uppercase_qualified_class_method_resolution_keeps_exact_confidence() {
    let (_dir, db) = temp_db_with_dimensions(3);
    let method = db
        .insert_symbol(&symbol("Parser.parse", "src/parser.py"))
        .unwrap();
    let caller = db.insert_symbol(&symbol("caller", "src/app.py")).unwrap();
    db.insert_edge(&edge(caller, "Parser.parse", None, "calls", None))
        .unwrap();

    assert_eq!(EdgeResolver::new(&db).resolve_all().unwrap(), 1);
    let resolved = db
        .get_edges_to_by_name("Parser.parse")
        .unwrap()
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(resolved.target_id, Some(method));
    assert_eq!(resolved.confidence, 1.0);
}

#[derive(Debug)]
struct FailingEmbedder;

impl Embedder for FailingEmbedder {
    fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Err(LoomError::EmbedderModel("qa forced failure".to_string()))
    }

    fn dimensions(&self) -> usize {
        3
    }
}

#[test]
fn qa_embedder_failure_does_not_mark_file_indexed_without_vectors() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("app.py"), "def alpha():\n    return 1\n").unwrap();
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let pipeline = IndexPipeline::new(config, Arc::clone(&db), Arc::new(FailingEmbedder));

    assert!(matches!(
        pipeline.full_index(),
        Err(LoomError::EmbedderModel(_))
    ));

    let stats = db.get_stats().unwrap();
    assert_eq!(stats.symbols, 0);
    assert_eq!(stats.vectors, 0);
    assert!(db.get_file_hash("app.py").unwrap().is_none());
}
