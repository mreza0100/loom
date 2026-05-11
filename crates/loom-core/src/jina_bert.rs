use candle_core::{DType, Device, Result, Tensor, D};
use candle_nn::{
    embedding, layer_norm, linear, linear_no_bias, Activation, Embedding, LayerNorm, Linear,
    Module, VarBuilder,
};
use candle_transformers::models::jina_bert::{Config, PositionEmbeddingType};

pub const DTYPE: DType = DType::F32;

#[derive(Clone, Debug)]
struct JinaBertEmbeddings {
    word_embeddings: Embedding,
    token_type_embeddings: Embedding,
    layer_norm: LayerNorm,
}

impl JinaBertEmbeddings {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        Ok(Self {
            word_embeddings: embedding(
                config.vocab_size,
                config.hidden_size,
                vb.pp("word_embeddings"),
            )?,
            token_type_embeddings: embedding(
                config.type_vocab_size,
                config.hidden_size,
                vb.pp("token_type_embeddings"),
            )?,
            layer_norm: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("LayerNorm"),
            )?,
        })
    }

    fn forward(&self, input_ids: &Tensor) -> Result<Tensor> {
        let (batch_size, seq_len) = input_ids.dims2()?;
        let input_embeddings = self.word_embeddings.forward(input_ids)?;
        let token_type_embeddings = Tensor::zeros(seq_len, DType::U32, input_ids.device())?
            .broadcast_left(batch_size)?
            .apply(&self.token_type_embeddings)?;
        let embeddings = (&input_embeddings + token_type_embeddings)?;
        self.layer_norm.forward(&embeddings)
    }
}

#[derive(Clone, Debug)]
struct JinaBertSelfAttention {
    query: Linear,
    key: Linear,
    value: Linear,
    layer_norm_q: LayerNorm,
    layer_norm_k: LayerNorm,
    num_attention_heads: usize,
    attention_head_size: usize,
}

impl JinaBertSelfAttention {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        let attention_head_size = config.hidden_size / config.num_attention_heads;
        let all_head_size = config.num_attention_heads * attention_head_size;
        Ok(Self {
            query: linear(config.hidden_size, all_head_size, vb.pp("query"))?,
            key: linear(config.hidden_size, all_head_size, vb.pp("key"))?,
            value: linear(config.hidden_size, all_head_size, vb.pp("value"))?,
            layer_norm_q: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("layer_norm_q"),
            )?,
            layer_norm_k: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("layer_norm_k"),
            )?,
            num_attention_heads: config.num_attention_heads,
            attention_head_size,
        })
    }

    fn transpose_for_scores(&self, xs: &Tensor) -> Result<Tensor> {
        let mut shape = xs.dims().to_vec();
        shape.pop();
        shape.push(self.num_attention_heads);
        shape.push(self.attention_head_size);
        xs.reshape(shape)?.transpose(1, 2)?.contiguous()
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
        alibi_bias: &Tensor,
    ) -> Result<Tensor> {
        let query_layer = self
            .query
            .forward(hidden_states)?
            .apply(&self.layer_norm_q)?;
        let key_layer = self.key.forward(hidden_states)?.apply(&self.layer_norm_k)?;
        let value_layer = self.value.forward(hidden_states)?;

        let query_layer = self.transpose_for_scores(&query_layer)?;
        let key_layer = self.transpose_for_scores(&key_layer)?;
        let value_layer = self.transpose_for_scores(&value_layer)?;

        let attention_scores = query_layer.matmul(&key_layer.t()?)?;
        let attention_scores = (attention_scores / (self.attention_head_size as f64).sqrt())?;
        let attention_scores = attention_scores.broadcast_add(alibi_bias)?;
        let attention_scores = if let Some(attention_mask) = attention_mask {
            attention_scores.broadcast_add(attention_mask)?
        } else {
            attention_scores
        };
        let attention_probs = candle_nn::ops::softmax_last_dim(&attention_scores)?;
        let context_layer = attention_probs.matmul(&value_layer)?;
        let context_layer = context_layer.transpose(1, 2)?.contiguous()?;
        context_layer.flatten_from(D::Minus2)
    }
}

#[derive(Clone, Debug)]
struct JinaBertSelfOutput {
    dense: Linear,
    layer_norm: LayerNorm,
}

impl JinaBertSelfOutput {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        Ok(Self {
            dense: linear(config.hidden_size, config.hidden_size, vb.pp("dense"))?,
            layer_norm: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("LayerNorm"),
            )?,
        })
    }

    fn forward(&self, hidden_states: &Tensor, input_tensor: &Tensor) -> Result<Tensor> {
        let hidden_states = self.dense.forward(hidden_states)?;
        self.layer_norm.forward(&(&hidden_states + input_tensor)?)
    }
}

#[derive(Clone, Debug)]
struct JinaBertAttention {
    self_attention: JinaBertSelfAttention,
    output: JinaBertSelfOutput,
}

impl JinaBertAttention {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        Ok(Self {
            self_attention: JinaBertSelfAttention::new(vb.pp("self"), config)?,
            output: JinaBertSelfOutput::new(vb.pp("output"), config)?,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
        alibi_bias: &Tensor,
    ) -> Result<Tensor> {
        let self_output = self
            .self_attention
            .forward(hidden_states, attention_mask, alibi_bias)?;
        self.output.forward(&self_output, hidden_states)
    }
}

#[derive(Clone, Debug)]
struct JinaBertGluMlp {
    up_gated_layer: Linear,
    down_layer: Linear,
    act: Activation,
    intermediate_size: usize,
}

impl JinaBertGluMlp {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        Ok(Self {
            up_gated_layer: linear_no_bias(
                config.hidden_size,
                config.intermediate_size * 2,
                vb.pp("up_gated_layer"),
            )?,
            down_layer: linear(
                config.intermediate_size,
                config.hidden_size,
                vb.pp("down_layer"),
            )?,
            act: config.hidden_act,
            intermediate_size: config.intermediate_size,
        })
    }

    fn forward(&self, hidden_states: &Tensor) -> Result<Tensor> {
        let hidden_mlp_states = self.up_gated_layer.forward(hidden_states)?;
        let up = hidden_mlp_states.narrow(D::Minus1, 0, self.intermediate_size)?;
        let gated =
            hidden_mlp_states.narrow(D::Minus1, self.intermediate_size, self.intermediate_size)?;
        let activated = gated.apply(&self.act)?;
        let hidden_mlp_states = up.broadcast_mul(&activated)?;
        self.down_layer.forward(&hidden_mlp_states)
    }
}

#[derive(Clone, Debug)]
struct JinaBertLayer {
    attention: JinaBertAttention,
    layer_norm_1: LayerNorm,
    layer_norm_2: LayerNorm,
    mlp: JinaBertGluMlp,
}

impl JinaBertLayer {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        Ok(Self {
            attention: JinaBertAttention::new(vb.pp("attention"), config)?,
            layer_norm_1: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("layer_norm_1"),
            )?,
            layer_norm_2: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("layer_norm_2"),
            )?,
            mlp: JinaBertGluMlp::new(vb.pp("mlp"), config)?,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
        alibi_bias: &Tensor,
    ) -> Result<Tensor> {
        let residual = hidden_states;
        let attention_output = self
            .attention
            .forward(hidden_states, attention_mask, alibi_bias)?;
        let residual = self
            .layer_norm_1
            .forward(&(&attention_output + residual)?)?;
        let mlp_output = self.mlp.forward(&residual)?;
        self.layer_norm_2.forward(&(&residual + &mlp_output)?)
    }
}

#[derive(Clone, Debug)]
struct JinaBertEncoder {
    layers: Vec<JinaBertLayer>,
    num_attention_heads: usize,
}

impl JinaBertEncoder {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        if config.position_embedding_type != PositionEmbeddingType::Alibi {
            candle_core::bail!("only alibi position embeddings are supported for Jina v2")
        }
        let layers = (0..config.num_hidden_layers)
            .map(|index| JinaBertLayer::new(vb.pp(format!("layer.{index}")), config))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            layers,
            num_attention_heads: config.num_attention_heads,
        })
    }

    fn forward(&self, hidden_states: &Tensor, attention_mask: Option<&Tensor>) -> Result<Tensor> {
        let seq_len = hidden_states.dim(1)?;
        let alibi_bias =
            build_alibi_bias(self.num_attention_heads, seq_len, hidden_states.device())?;
        let mut hidden_states = hidden_states.clone();
        for layer in &self.layers {
            hidden_states = layer.forward(&hidden_states, attention_mask, &alibi_bias)?;
        }
        Ok(hidden_states)
    }
}

#[derive(Clone, Debug)]
pub struct JinaBertModel {
    embeddings: JinaBertEmbeddings,
    encoder: JinaBertEncoder,
}

impl JinaBertModel {
    pub fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        Ok(Self {
            embeddings: JinaBertEmbeddings::new(vb.pp("embeddings"), config)?,
            encoder: JinaBertEncoder::new(vb.pp("encoder"), config)?,
        })
    }

    pub fn forward(&self, input_ids: &Tensor, attention_mask: Option<&Tensor>) -> Result<Tensor> {
        let embedding_output = self.embeddings.forward(input_ids)?;
        self.encoder.forward(&embedding_output, attention_mask)
    }
}

fn build_alibi_bias(num_heads: usize, seq_len: usize, device: &Device) -> Result<Tensor> {
    let positions = Tensor::arange(0, seq_len as i64, device)?.to_dtype(DType::F32)?;
    let context_position = positions.reshape((seq_len, 1))?;
    let memory_position = positions.reshape((1, seq_len))?;
    let relative_position = memory_position.broadcast_sub(&context_position)?.abs()?;
    let relative_position = relative_position.reshape((1, 1, seq_len, seq_len))?;
    let slopes = alibi_head_slopes(num_heads)
        .into_iter()
        .map(|slope| -slope)
        .collect::<Vec<_>>();
    let slopes = Tensor::new(slopes, device)?.reshape((1, num_heads, 1, 1))?;
    relative_position.broadcast_mul(&slopes)
}

fn alibi_head_slopes(num_heads: usize) -> Vec<f32> {
    fn slopes_power_of_two(count: usize) -> Vec<f32> {
        let start = 2_f32.powf(-(2_f32.powf(-((count as f32).log2() - 3.0))));
        (0..count)
            .map(|index| start * start.powi(index as i32))
            .collect()
    }

    if num_heads.is_power_of_two() {
        return slopes_power_of_two(num_heads);
    }
    let closest_power = 1_usize << (usize::BITS - 1 - num_heads.leading_zeros());
    let mut slopes = slopes_power_of_two(closest_power);
    slopes.extend(
        slopes_power_of_two(closest_power * 2)
            .into_iter()
            .step_by(2)
            .take(num_heads - closest_power),
    );
    slopes
}
