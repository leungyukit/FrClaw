//! Embedding 抽象。
//!
//! 两种来源：
//! 1. **Provider embedding**：调 LLM provider 的 `/embeddings` endpoint
//! 2. **Hash baseline**：纯本地的 signed hashing TF (256-dim)，零网络
//!
//! Hash baseline 跟 `fastembed-mini` / `sentence-transformers.js` 的离线模式思路一致：
//! tokenize → murmur 风格 hash → signed count → L2 归一化。质量比不上真模型，
//! 但对「个人 RAG 知识库」「关键词召回」完全够用，而且开箱即用。

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// 向量维度（hash baseline 固定 256）。
pub const HASH_DIM: usize = 256;

/// 归一化后的 embedding（f32，|v|=1）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Embedding {
    pub vec: Vec<f32>,
    pub dim: usize,
    pub origin: EmbeddingOrigin,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EmbeddingOrigin {
    /// 调用 provider /embeddings 端点
    Provider,
    /// 本地 hashing fallback
    Hash,
}

impl Embedding {
    pub fn zeros() -> Self {
        Self { vec: vec![0.0; HASH_DIM], dim: HASH_DIM, origin: EmbeddingOrigin::Hash }
    }

    /// 余弦相似度（已归一化 → 点积）。
    pub fn cosine(&self, other: &Embedding) -> f32 {
        if self.dim != other.dim {
            return 0.0;
        }
        self.vec.iter().zip(&other.vec).map(|(a, b)| a * b).sum()
    }
}

/// Embedder 抽象。
#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Embedding>;
    fn name(&self) -> &str;
    fn dim(&self) -> usize;
}

/// 纯本地 hashing embedder。
pub struct HashEmbedder;

impl HashEmbedder {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Embedder for HashEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        Ok(Embedding { vec: hash_embed(text), dim: HASH_DIM, origin: EmbeddingOrigin::Hash })
    }
    fn name(&self) -> &str {
        "hash"
    }
    fn dim(&self) -> usize {
        HASH_DIM
    }
}

/// 调 provider 的 `/embeddings` 端点的 embedder。
pub struct ProviderEmbedder {
    name: String,
    base_url: String,
    api_key: String,
    model: String,
    dim: usize,
    client: reqwest::Client,
}

impl ProviderEmbedder {
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        dim: usize,
    ) -> Self {
        Self {
            name: name.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            dim,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait::async_trait]
impl Embedder for ProviderEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "input": text,
        });
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("embedding request failed: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("embedding endpoint returned {status}: {body}"));
        }
        let parsed: EmbeddingResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("embedding response parse failed: {e}"))?;
        let vec = parsed
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("embedding response has no data"))?
            .embedding;
        // 归一化
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        let vec = if norm > 0.0 {
            vec.into_iter().map(|x| x / norm).collect()
        } else {
            vec
        };
        Ok(Embedding { vec, dim: self.dim, origin: EmbeddingOrigin::Provider })
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn dim(&self) -> usize {
        self.dim
    }
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

/// 自动选：先 provider，再 hash。
pub struct FallbackEmbedder {
    pub primary: Option<Arc<dyn Embedder>>,
    pub fallback: Arc<HashEmbedder>,
}

impl FallbackEmbedder {
    pub fn new(primary: Option<Arc<dyn Embedder>>) -> Self {
        Self { primary, fallback: Arc::new(HashEmbedder::new()) }
    }
}

#[async_trait::async_trait]
impl Embedder for FallbackEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        if let Some(p) = &self.primary {
            match p.embed(text).await {
                Ok(e) => return Ok(e),
                Err(_) => {
                    // 静默 fallback
                }
            }
        }
        self.fallback.embed(text).await
    }
    fn name(&self) -> &str {
        "fallback"
    }
    fn dim(&self) -> usize {
        HASH_DIM
    }
}

/// 纯本地 hashing embedder 函数实现。
///
/// 步骤：
/// 1. 小写化 + Unicode word tokenize
/// 2. 每个 token：sha256 → 拆成 8 个 u32 → 取模 256（signed 决定正负号）
/// 3. 全局 L2 归一化
pub fn hash_embed(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; HASH_DIM];
    let lower = text.to_lowercase();
    for token in tokenize(&lower) {
        if token.is_empty() {
            continue;
        }
        // 让 sha256 出多个 hash bucket
        let mut h = Sha256::new();
        h.update(token.as_bytes());
        let bytes = h.finalize();
        // 用 8 个 u32，每个走 (bytes, sign_bit) → bucket
        for chunk in bytes.chunks(4).take(8) {
            if chunk.len() < 4 {
                break;
            }
            let n = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let bucket = (n % (HASH_DIM as u32)) as usize;
            let sign = if n & 0x8000_0000 != 0 { 1.0 } else { -1.0 };
            v[bucket] += sign;
        }
    }
    // 归一化
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// 极简 Unicode tokenizer。
/// - 拆出连续的中英文字符作为 token
/// - 其余（标点、空格）作为分隔符
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_embed_normalized() {
        let v = hash_embed("hello world hello world");
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-4, "norm = {n}");
    }

    #[test]
    fn hash_embed_similar_texts_close() {
        let a = hash_embed("rust programming language");
        let b = hash_embed("rust programming language");
        let c = hash_embed("cooking recipe for chicken");
        let ab: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        let ac: f32 = a.iter().zip(&c).map(|(x, y)| x * y).sum();
        assert!(ab > ac, "similar texts should have higher cosine: {ab} vs {ac}");
    }

    #[test]
    fn hash_embed_chinese() {
        let v = hash_embed("FrClaw 是一个 AI 助手");
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-4);
    }

    #[test]
    fn tokenizer_handles_cjk() {
        let t = tokenize("Hello FrClaw world_2026");
        assert!(t.contains(&"Hello".to_string()));
        assert!(t.contains(&"FrClaw".to_string()));
        assert!(t.contains(&"world_2026".to_string()));
    }

    #[test]
    fn embedding_cosine() {
        let a = Embedding { vec: hash_embed("rust"), dim: HASH_DIM, origin: EmbeddingOrigin::Hash };
        let b = Embedding { vec: hash_embed("rust language"), dim: HASH_DIM, origin: EmbeddingOrigin::Hash };
        let c = Embedding { vec: hash_embed("cooking"), dim: HASH_DIM, origin: EmbeddingOrigin::Hash };
        let ab = a.cosine(&b);
        let ac = a.cosine(&c);
        assert!(ab > ac, "cosine rust vs rust-language = {ab}, vs cooking = {ac}");
    }
}
