use loom_core::{
    embedder::{
        build_symbol_text, DefaultEmbedder, Embedder, HashingEmbedder, ModelFiles, ModelSource,
    },
    EmbeddingBackendConfig, LoomConfig, LoomError, Result,
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
fn symbol_text_matches_rust_contract() {
    assert_eq!(
        build_symbol_text("alpha", "function", "function alpha() {}"),
        "function alpha\nfunction alpha() {}"
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

#[test]
fn default_embedder_hashing_mode_skips_candle_loader() {
    let mut config = LoomConfig::default_for_target(".");
    config.embedding_backend = EmbeddingBackendConfig::Hashing;
    config.embedding_dimensions = 8;

    let embedder = DefaultEmbedder::from_config_with_candle_loader(&config, || {
        panic!("hashing mode must not initialize Candle")
    })
    .unwrap();
    let status = embedder.status();

    assert_eq!(status.backend, "hashing");
    assert!(!status.degraded);
    assert_eq!(status.dimensions, 8);
    assert_eq!(status.model, None);
}

#[test]
fn default_embedder_candle_failure_is_strict_by_default() {
    let config = LoomConfig::default_for_target(".");

    let result = DefaultEmbedder::from_config_with_candle_loader(&config, || {
        Err(LoomError::EmbedderModel("boom".to_string()))
    });

    assert!(matches!(result, Err(LoomError::EmbedderModel(_))));
}

#[test]
fn default_embedder_explicit_fallback_reports_degraded_hashing() {
    let mut config = LoomConfig::default_for_target(".");
    config.embedding_dimensions = 8;
    config.allow_hashing_embedder_fallback = true;

    let embedder = DefaultEmbedder::from_config_with_candle_loader(&config, || {
        Err(LoomError::EmbedderModel("boom".to_string()))
    })
    .unwrap();
    let status = embedder.status();

    assert_eq!(status.backend, "hashing");
    assert!(status.degraded);
    assert_eq!(status.dimensions, 8);
    assert_eq!(status.model, None);
    assert_eq!(
        embedder.fingerprint(),
        "embedder=hashing;degraded=true;model=none;dims=8"
    );
    assert_ne!(embedder.fingerprint(), config.embedding_fingerprint());
}

#[test]
fn live_jina_candle_smoke_when_enabled() {
    if !matches!(std::env::var("LOOM_LIVE_JINA_SMOKE").as_deref(), Ok("1")) {
        eprintln!("skipping live Jina smoke; set LOOM_LIVE_JINA_SMOKE=1 to run");
        return;
    }

    let config = LoomConfig::default_for_target(".");
    let embedder = DefaultEmbedder::from_config(&config).unwrap();
    let status = embedder.status();

    assert_eq!(status.backend, "candle");
    assert!(!status.degraded);
    assert_eq!(status.dimensions, 768);
    assert_eq!(
        status.model.as_deref(),
        Some("jinaai/jina-embeddings-v2-base-code")
    );

    let texts = vec![
        "function parseConfig(input) { return JSON.parse(input); }".to_string(),
        "function parseConfig(input) { return JSON.parse(input); }".to_string(),
        "button.primary { color: rebeccapurple; }".to_string(),
    ];
    let embeddings = embedder.embed(&texts).unwrap();

    assert_eq!(embeddings.len(), 3);
    for embedding in &embeddings {
        assert_eq!(embedding.len(), 768);
        assert!(embedding.iter().all(|value| value.is_finite()));
        let norm = embedding
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        assert!((norm - 1.0).abs() < 0.000_1);
    }
    assert!(dot(&embeddings[0], &embeddings[1]) > dot(&embeddings[0], &embeddings[2]));
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum()
}
