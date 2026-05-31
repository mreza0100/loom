use loom_core::{
    embedder::Embedder,
    graph::SymbolGraph,
    indexer::{index_fingerprint, IndexPipeline},
    store::LoomDb,
    LoomConfig, LoomError, Result, SearchEngine, VectorBackendConfig,
};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

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
                vector[0] = text.len() as f32;
                vector
            })
            .collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[derive(Debug)]
struct RecordingEmbedder {
    dimensions: usize,
    batch_sizes: Arc<Mutex<Vec<usize>>>,
}

impl Embedder for RecordingEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.batch_sizes.lock().unwrap().push(texts.len());
        Ok(texts
            .iter()
            .map(|text| {
                let mut vector = vec![0.0; self.dimensions];
                vector[0] = text.len() as f32;
                vector
            })
            .collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[test]
fn full_index_indexes_changed_files_and_skips_unchanged_files() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("app.ts");
    fs::write(&file, "function alpha() {\n  return 1;\n}\n").unwrap();
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let pipeline = IndexPipeline::new(
        config,
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
    );

    let first = pipeline.full_index().unwrap();
    assert_eq!(first.indexed, 1);
    assert_eq!(first.symbols, 1);
    assert_eq!(first.embeddings, 1);

    let second = pipeline.full_index().unwrap();
    assert_eq!(second.indexed, 0);
    assert_eq!(second.skipped, 1);
    assert_eq!(db.get_stats().unwrap().vectors, 1);
}

#[test]
fn full_index_removes_stale_deleted_files() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("app.ts");
    fs::write(&file, "function alpha() {\n  return 1;\n}\n").unwrap();
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let pipeline = IndexPipeline::new(
        config,
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
    );
    pipeline.full_index().unwrap();
    fs::remove_file(&file).unwrap();

    let result = pipeline.full_index().unwrap();
    assert_eq!(result.deleted, 1);
    assert_eq!(db.get_stats().unwrap().symbols, 0);
    assert!(db.get_file_hash("app.ts").unwrap().is_none());
}

#[test]
fn stats_report_actual_unindexed_and_changed_files_as_stale() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("app.ts");
    fs::write(&file, "function alpha() {\n  return 1;\n}\n").unwrap();
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());

    assert_eq!(db.get_stats().unwrap().stale_files, 1);

    let pipeline = IndexPipeline::new(
        config,
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
    );
    pipeline.full_index().unwrap();
    assert_eq!(db.get_stats().unwrap().stale_files, 0);

    fs::write(&file, "function alpha() {\n  return 2;\n}\n").unwrap();
    assert_eq!(db.get_stats().unwrap().stale_files, 1);

    pipeline.full_index().unwrap();
    assert_eq!(db.get_stats().unwrap().stale_files, 0);

    fs::remove_file(&file).unwrap();
    assert_eq!(db.get_stats().unwrap().stale_files, 1);
}

#[test]
fn full_index_handles_more_files_than_old_parser_channel_bound() {
    let dir = tempdir().unwrap();
    for index in 0..80 {
        fs::write(
            dir.path().join(format!("module_{index}.ts")),
            format!("function symbol_{index}() {{\n  return {index};\n}}\n"),
        )
        .unwrap();
    }
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let pipeline = IndexPipeline::new(config, db, Arc::new(MockEmbedder { dimensions: 3 }));

    let result = pipeline.full_index().unwrap();
    assert_eq!(result.indexed, 80);
    assert_eq!(result.symbols, 80);
}

#[test]
fn full_index_streams_large_inputs_in_bounded_embedding_batches() {
    let dir = tempdir().unwrap();
    for index in 0..80 {
        fs::write(
            dir.path().join(format!("module_{index}.ts")),
            format!("function symbol_{index}() {{\n  return {index};\n}}\n"),
        )
        .unwrap();
    }
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let batch_sizes = Arc::new(Mutex::new(Vec::new()));
    let pipeline = IndexPipeline::new(
        config,
        db,
        Arc::new(RecordingEmbedder {
            dimensions: 3,
            batch_sizes: Arc::clone(&batch_sizes),
        }),
    );

    let result = pipeline.full_index().unwrap();
    let batch_sizes = batch_sizes.lock().unwrap();

    assert_eq!(result.indexed, 80);
    assert_eq!(result.symbols, 80);
    assert!(batch_sizes.len() >= 3);
    assert!(batch_sizes.iter().all(|size| *size <= 32));
}

#[test]
fn full_index_resolves_receiver_method_calls_by_callee_name_for_impact() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("a.ts"),
        "export class A {\n  getFont() {\n    return 'mono';\n  }\n}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("b.ts"),
        "import { A } from './a';\nexport class B {\n  constructor(private a: A) {}\n  doWork() {\n    return this.a.getFont();\n  }\n}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("c.ts"),
        "import { A } from './a';\nexport class C {\n  render(a: A) {\n    return a.getFont();\n  }\n}\n",
    )
    .unwrap();

    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let pipeline = IndexPipeline::new(
        config.clone(),
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
    );
    let result = pipeline.full_index().unwrap();

    assert_eq!(result.errors, 0);
    let graph = Arc::new(SymbolGraph::build_from_db(&db).unwrap());
    let engine = SearchEngine::new(
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
        Some(graph),
        config,
    );

    let impact = engine.impact("getFont", None, Some("method")).unwrap();
    let impacted = impact
        .results
        .iter()
        .map(|hit| hit.symbol.name.as_str())
        .collect::<Vec<_>>();

    assert!(impacted.contains(&"B.doWork"), "{impacted:?}");
    assert!(impacted.contains(&"C.render"), "{impacted:?}");
    assert!(impact.results.iter().any(|hit| hit
        .provenance
        .iter()
        .any(|item| item.relationship == "calls")));
}

#[test]
fn full_index_rebuilds_vectors_when_backend_changes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("app.ts");
    fs::write(&file, "function alpha() {\n  return 1;\n}\n").unwrap();
    let mut blob_config = LoomConfig::default_for_target(dir.path());
    blob_config.embedding_dimensions = 3;
    blob_config.enable_git_analysis = false;
    blob_config.vector_backend = VectorBackendConfig::Blob;
    let blob_db = Arc::new(LoomDb::open(blob_config.clone()).unwrap());
    let blob_pipeline = IndexPipeline::new(
        blob_config,
        Arc::clone(&blob_db),
        Arc::new(MockEmbedder { dimensions: 3 }),
    );
    assert_eq!(blob_pipeline.full_index().unwrap().indexed, 1);
    assert_eq!(blob_db.get_stats().unwrap().vectors, 1);

    let mut sqlite_config = LoomConfig::default_for_target(dir.path());
    sqlite_config.embedding_dimensions = 3;
    sqlite_config.enable_git_analysis = false;
    sqlite_config.vector_backend = VectorBackendConfig::SqliteVec;
    let sqlite_db = Arc::new(LoomDb::open(sqlite_config.clone()).unwrap());
    assert_eq!(sqlite_db.get_stats().unwrap().vectors, 0);
    let sqlite_pipeline = IndexPipeline::new(
        sqlite_config,
        Arc::clone(&sqlite_db),
        Arc::new(MockEmbedder { dimensions: 3 }),
    );

    let rebuilt = sqlite_pipeline.full_index().unwrap();

    assert_eq!(rebuilt.indexed, 1);
    assert_eq!(rebuilt.skipped, 0);
    assert_eq!(sqlite_db.get_stats().unwrap().vectors, 1);
    assert!(sqlite_db
        .file_index_is_fresh("app.ts", &walk_hash(&file), &mock_index_fingerprint(3))
        .unwrap());
}

#[test]
fn full_index_rebuilds_vectors_when_embedding_fingerprint_changes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("app.ts");
    fs::write(&file, "function alpha() {\n  return 1;\n}\n").unwrap();
    let mut first_config = LoomConfig::default_for_target(dir.path());
    first_config.embedding_dimensions = 3;
    first_config.enable_git_analysis = false;
    first_config.vector_backend = VectorBackendConfig::Blob;
    let db = Arc::new(LoomDb::open(first_config.clone()).unwrap());
    let first_pipeline = IndexPipeline::new(
        first_config,
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
    );
    assert_eq!(first_pipeline.full_index().unwrap().indexed, 1);
    assert!(db
        .file_index_is_fresh("app.ts", &walk_hash(&file), &mock_index_fingerprint(3))
        .unwrap());

    let mut second_config = LoomConfig::default_for_target(dir.path());
    second_config.embedding_dimensions = 4;
    second_config.enable_git_analysis = false;
    second_config.vector_backend = VectorBackendConfig::Blob;
    let reopened = Arc::new(LoomDb::open(second_config.clone()).unwrap());
    assert!(!reopened
        .file_index_is_fresh("app.ts", &walk_hash(&file), &mock_index_fingerprint(4))
        .unwrap());
    let second_pipeline = IndexPipeline::new(
        second_config,
        Arc::clone(&reopened),
        Arc::new(MockEmbedder { dimensions: 4 }),
    );

    let rebuilt = second_pipeline.full_index().unwrap();

    assert_eq!(rebuilt.indexed, 1);
    assert_eq!(rebuilt.skipped, 0);
    assert_eq!(reopened.get_stats().unwrap().vectors, 1);
    assert!(reopened
        .file_index_is_fresh("app.ts", &walk_hash(&file), &mock_index_fingerprint(4))
        .unwrap());
}

#[test]
fn incremental_delete_removes_symbols_vectors_and_hash() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("app.ts");
    fs::write(&file, "function alpha() {\n  return 1;\n}\n").unwrap();
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let pipeline = IndexPipeline::new(
        config,
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
    );
    pipeline.full_index().unwrap();
    fs::remove_file(&file).unwrap();

    let result = pipeline.incremental_index([file]).unwrap();
    assert_eq!(result.deleted, 1);
    let stats = db.get_stats().unwrap();
    assert_eq!(stats.symbols, 0);
    assert_eq!(stats.vectors, 0);
    assert!(db.get_file_hash("app.ts").unwrap().is_none());
}

fn walk_hash(path: &std::path::Path) -> String {
    loom_core::indexer::walk::hash_file(path).unwrap()
}

fn mock_fingerprint(dimensions: usize) -> String {
    MockEmbedder { dimensions }.fingerprint()
}

fn mock_index_fingerprint(dimensions: usize) -> String {
    index_fingerprint(&mock_fingerprint(dimensions))
}

#[test]
fn incremental_rename_removes_old_path_and_indexes_new_path() {
    let dir = tempdir().unwrap();
    let old_file = dir.path().join("old.ts");
    let new_file = dir.path().join("new.ts");
    fs::write(&old_file, "function renamed_symbol() {\n  return 1;\n}\n").unwrap();
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let pipeline = IndexPipeline::new(
        config,
        Arc::clone(&db),
        Arc::new(MockEmbedder { dimensions: 3 }),
    );
    pipeline.full_index().unwrap();
    fs::rename(&old_file, &new_file).unwrap();

    let result = pipeline
        .incremental_index([old_file.clone(), new_file.clone()])
        .unwrap();

    assert_eq!(result.deleted, 1);
    assert_eq!(result.indexed, 1);
    let stats = db.get_stats().unwrap();
    assert_eq!(stats.files, 1);
    assert_eq!(stats.symbols, 1);
    assert_eq!(stats.vectors, 1);
    assert!(db.get_file_hash("old.ts").unwrap().is_none());
    assert!(db.get_file_hash("new.ts").unwrap().is_some());
    let symbols = db
        .get_symbol_by_name("renamed_symbol", None)
        .unwrap()
        .into_iter()
        .map(|symbol| symbol.file)
        .collect::<Vec<_>>();
    assert_eq!(symbols, vec!["new.ts"]);
}

#[test]
fn incremental_index_rejects_relative_path_escape() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("target");
    fs::create_dir(&target).unwrap();
    fs::write(
        dir.path().join("outside.ts"),
        "function outside() {\n  return 1;\n}\n",
    )
    .unwrap();
    let mut config = LoomConfig::default_for_target(&target);
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let pipeline = IndexPipeline::new(config, db, Arc::new(MockEmbedder { dimensions: 3 }));

    let error = pipeline
        .incremental_index([std::path::PathBuf::from("../outside.ts")])
        .unwrap_err();
    assert!(matches!(error, LoomError::IndexerPath(_)));
}

#[derive(Debug)]
struct BadEmbedder;

impl Embedder for BadEmbedder {
    fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Err(LoomError::EmbedderModel("test failure".to_string()))
    }

    fn dimensions(&self) -> usize {
        3
    }
}

#[test]
fn embedder_failure_is_batch_fatal() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("app.ts"),
        "function alpha() {\n  return 1;\n}\n",
    )
    .unwrap();
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let pipeline = IndexPipeline::new(config, db, Arc::new(BadEmbedder));

    let error = pipeline.full_index().unwrap_err();
    assert!(matches!(error, LoomError::EmbedderModel(_)));
}
