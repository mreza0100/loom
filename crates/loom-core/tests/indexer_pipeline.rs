use loom_core::{
    embedder::Embedder, indexer::IndexPipeline, store::LoomDb, LoomConfig, LoomError, Result,
};
use std::fs;
use std::sync::Arc;
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

#[test]
fn full_index_indexes_changed_files_and_skips_unchanged_files() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("app.py");
    fs::write(&file, "def alpha():\n    return 1\n").unwrap();
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
fn incremental_delete_removes_symbols_vectors_and_hash() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("app.py");
    fs::write(&file, "def alpha():\n    return 1\n").unwrap();
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
    assert!(db.get_file_hash("app.py").unwrap().is_none());
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
    fs::write(dir.path().join("app.py"), "def alpha():\n    return 1\n").unwrap();
    let mut config = LoomConfig::default_for_target(dir.path());
    config.embedding_dimensions = 3;
    config.enable_git_analysis = false;
    let db = Arc::new(LoomDb::open(config.clone()).unwrap());
    let pipeline = IndexPipeline::new(config, db, Arc::new(BadEmbedder));

    let error = pipeline.full_index().unwrap_err();
    assert!(matches!(error, LoomError::EmbedderModel(_)));
}
