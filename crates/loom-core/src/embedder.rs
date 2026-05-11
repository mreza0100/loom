use crate::error::{LoomError, Result};
use candle_core::{Device, Tensor};
use candle_nn::{Module, VarBuilder};
use candle_transformers::models::jina_bert::{BertModel, Config, DTYPE};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokenizers::utils::truncation::{TruncationDirection, TruncationParams, TruncationStrategy};
use tokenizers::Tokenizer;
use tracing::warn;

const REQUIRED_MODEL_FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];
const MAX_TOKEN_LENGTH: usize = 8_192;

pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;

    fn embed_single(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed(&[text.to_string()])?;
        embeddings.into_iter().next().ok_or_else(|| {
            LoomError::EmbedderModel("single embedding request returned no vectors".to_string())
        })
    }

    fn dimensions(&self) -> usize;
}

pub trait ModelSource: Send + Sync {
    fn ensure_model_files(&self, repo: &str, cache_dir: &Path) -> Result<ModelFiles>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFiles {
    pub config: PathBuf,
    pub tokenizer: PathBuf,
    pub weights: PathBuf,
}

#[derive(Debug, Clone)]
pub struct HfHubModelSource;

impl ModelSource for HfHubModelSource {
    fn ensure_model_files(&self, repo: &str, cache_dir: &Path) -> Result<ModelFiles> {
        let api = hf_hub::api::sync::ApiBuilder::new()
            .with_cache_dir(cache_dir.to_path_buf())
            .build()
            .map_err(|source| LoomError::EmbedderDownload(source.to_string()))?;
        let model = api.model(repo.to_string());
        let mut paths = Vec::with_capacity(REQUIRED_MODEL_FILES.len());
        for filename in REQUIRED_MODEL_FILES {
            let path = model
                .get(filename)
                .map_err(|source| LoomError::EmbedderDownload(source.to_string()))?;
            paths.push(path);
        }
        Ok(ModelFiles {
            config: paths.remove(0),
            tokenizer: paths.remove(0),
            weights: paths.remove(0),
        })
    }
}

pub struct CandleEmbedder<S: ModelSource = HfHubModelSource> {
    dimensions: usize,
    tokenizer: Tokenizer,
    device: Device,
    model: BertModel,
    pad_token_id: u32,
    _model_files: ModelFiles,
    _source: Arc<S>,
}

impl CandleEmbedder<HfHubModelSource> {
    pub fn from_config(config: &crate::config::LoomConfig) -> Result<Self> {
        Self::new(
            config.embedding_model.clone(),
            config.model_cache_dir.clone(),
            config.embedding_dimensions,
            Arc::new(HfHubModelSource),
        )
    }
}

impl<S: ModelSource> CandleEmbedder<S> {
    pub fn new(
        model_repo: String,
        cache_dir: PathBuf,
        dimensions: usize,
        source: Arc<S>,
    ) -> Result<Self> {
        let model_files = source.ensure_model_files(&model_repo, &cache_dir)?;
        let tokenizer = Tokenizer::from_file(&model_files.tokenizer)
            .map_err(|source| LoomError::EmbedderTokenizer(source.to_string()))?;
        if !model_files.config.exists() || !model_files.weights.exists() {
            return Err(LoomError::EmbedderModel(format!(
                "model files missing under {}",
                cache_dir.display()
            )));
        }
        let device = select_device()?;
        let config_raw = std::fs::read_to_string(&model_files.config)
            .map_err(|source| LoomError::EmbedderModel(source.to_string()))?;
        let model_config: Config = serde_json::from_str(&config_raw)
            .map_err(|source| LoomError::EmbedderModel(source.to_string()))?;
        if model_config.hidden_size != dimensions {
            return Err(LoomError::EmbeddingDimension {
                expected: dimensions,
                actual: model_config.hidden_size,
            });
        }
        let model = load_jina_model(&model_files.weights, &device, &model_config)?;
        let pad_token_id = u32::try_from(model_config.pad_token_id)
            .map_err(|source| LoomError::EmbedderModel(source.to_string()))?;
        Ok(Self {
            dimensions,
            tokenizer,
            device,
            model,
            pad_token_id,
            _model_files: model_files,
            _source: source,
        })
    }

    fn validate_embeddings(&self, embeddings: &[Vec<f32>]) -> Result<()> {
        for embedding in embeddings {
            if embedding.len() != self.dimensions {
                return Err(LoomError::EmbeddingDimension {
                    expected: self.dimensions,
                    actual: embedding.len(),
                });
            }
        }
        Ok(())
    }
}

impl<S: ModelSource> Embedder for CandleEmbedder<S> {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut tokenizer = self.tokenizer.clone();
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MAX_TOKEN_LENGTH,
                strategy: TruncationStrategy::LongestFirst,
                stride: 0,
                direction: TruncationDirection::Right,
            }))
            .map_err(|source| LoomError::EmbedderTokenizer(source.to_string()))?;
        let encodings = tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|source| LoomError::EmbedderTokenizer(source.to_string()))?;
        let max_len = encodings
            .iter()
            .map(|encoding| encoding.get_ids().len())
            .max()
            .unwrap_or(0);
        let mut ids = Vec::with_capacity(encodings.len());
        let mut masks = Vec::with_capacity(encodings.len());
        for encoding in encodings {
            let mut row = encoding.get_ids().to_vec();
            let mut mask = vec![1.0_f32; row.len()];
            row.resize(max_len, self.pad_token_id);
            mask.resize(max_len, 0.0);
            ids.push(row);
            masks.push(mask);
        }
        let input_ids = Tensor::new(ids, &self.device)
            .map_err(|source| LoomError::EmbedderModel(source.to_string()))?;
        let sequence = self
            .model
            .forward(&input_ids)
            .map_err(|source| LoomError::EmbedderModel(source.to_string()))?;
        let values = sequence
            .to_vec3::<f32>()
            .map_err(|source| LoomError::EmbedderModel(source.to_string()))?;
        let mut embeddings = mean_pool(values, &masks, self.dimensions)?;
        self.validate_embeddings(&embeddings)?;
        for embedding in &mut embeddings {
            l2_normalize(embedding);
        }
        Ok(embeddings)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

pub fn build_symbol_text(name: &str, kind: &str, context: &str) -> String {
    format!("{kind} {name}\n{context}")
}

fn load_jina_model(weights: &Path, device: &Device, config: &Config) -> Result<BertModel> {
    let paths = [weights];
    // SAFETY: Candle maps immutable safetensors files and owns the mmap backend for the
    // VarBuilder lifetime. Loom only passes local cache paths returned by ModelSource.
    let var_builder = unsafe { VarBuilder::from_mmaped_safetensors(&paths, DTYPE, device) }
        .map_err(|source| LoomError::EmbedderModel(source.to_string()))?;
    BertModel::new(var_builder, config)
        .map_err(|source| LoomError::EmbedderModel(source.to_string()))
}

fn mean_pool(
    values: Vec<Vec<Vec<f32>>>,
    masks: &[Vec<f32>],
    dimensions: usize,
) -> Result<Vec<Vec<f32>>> {
    let mut embeddings = Vec::with_capacity(values.len());
    for (token_vectors, mask) in values.into_iter().zip(masks.iter()) {
        let mut pooled = vec![0.0_f32; dimensions];
        let mut denominator = 0.0_f32;
        for (token_vector, token_mask) in token_vectors.into_iter().zip(mask.iter()) {
            if *token_mask == 0.0 {
                continue;
            }
            if token_vector.len() != dimensions {
                return Err(LoomError::EmbeddingDimension {
                    expected: dimensions,
                    actual: token_vector.len(),
                });
            }
            denominator += *token_mask;
            for (slot, value) in pooled.iter_mut().zip(token_vector.iter()) {
                *slot += *value;
            }
        }
        if denominator > 0.0 {
            for value in &mut pooled {
                *value /= denominator;
            }
        }
        embeddings.push(pooled);
    }
    Ok(embeddings)
}

fn select_device() -> Result<Device> {
    #[cfg(target_os = "macos")]
    {
        match Device::new_metal(0) {
            Ok(device) => return Ok(device),
            Err(source) => {
                warn!(error = %source, "Metal unavailable; falling back to CPU");
            }
        }
    }
    Ok(Device::Cpu)
}

fn l2_normalize(vector: &mut [f32]) {
    let norm = vector
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    if norm == 0.0 {
        return;
    }
    for value in vector {
        *value = (f64::from(*value) / norm) as f32;
    }
}
