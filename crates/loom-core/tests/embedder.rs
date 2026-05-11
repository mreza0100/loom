use loom_core::{
    embedder::{build_symbol_text, ModelFiles, ModelSource},
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
