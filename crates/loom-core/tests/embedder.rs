use loom_core::{
    embedder::{build_symbol_text, Embedder, HashingEmbedder, ModelFiles, ModelSource},
    Result,
};
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct LocalModelSource {
    files: ModelFiles,
}

impl ModelSource for LocalModelSource {
    fn ensure_model_files(&self, _repo: &str, _cache_dir: &Path) -> Result<ModelFiles> {
        Ok(self.files.clone())
    }
}

#[test]
fn symbol_text_matches_python_contract() {
    assert_eq!(
        build_symbol_text("alpha", "function", "def alpha(): pass"),
        "function alpha\ndef alpha(): pass"
    );
}

#[test]
fn model_source_boundary_is_mockable_without_network() {
    let files = ModelFiles {
        config: PathBuf::from("config.json"),
        tokenizer: PathBuf::from("tokenizer.json"),
        weights: PathBuf::from("model.safetensors"),
    };
    let source = LocalModelSource {
        files: files.clone(),
    };

    assert_eq!(
        source
            .ensure_model_files("mock/model", Path::new("/tmp/cache"))
            .unwrap(),
        files
    );
}

#[test]
fn hashing_embedder_is_deterministic_and_normalized() {
    let embedder = HashingEmbedder::new(32);
    let first = embedder
        .embed_single("function compile Compiler webpack")
        .unwrap();
    let second = embedder
        .embed_single("function compile Compiler webpack")
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(first.len(), 32);
    let norm = first.iter().map(|value| value * value).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 0.000_01);
}

#[test]
fn hashing_embedder_handles_empty_text_with_zero_vector() {
    let embedder = HashingEmbedder::new(8);
    let vector = embedder.embed_single("").unwrap();

    assert_eq!(vector, vec![0.0; 8]);
}
