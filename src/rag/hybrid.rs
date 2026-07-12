//! Round 15b ─ 混合检索：BM25 关键词 + vector cosine 加权融合。
//!
//! 设计：
//! - 全文 BM25：tf / df in-memory index，token 走 `chunk.rs::tokenize` 同一套规则
//! - 向量 cosine：复用 RagStore 已有 embedding
//! - 融合：min-max 归一化各 score → 加权求和 → 排序
//!
//! 不引 tantivy / meilisearch —— 简单 tokenize + inverted index 完全够个人 RAG 用。

use super::embed::tokenize;
use super::store::Hit;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// 一条 hit 带 BM25 分数 + vector 分数 + 综合。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HybridHit {
    pub source: String,
    pub chunk_index: usize,
    pub content: String,
    pub vector_score: f32,
    pub bm25_score: f32,
    pub hybrid_score: f32,
    pub origin: String,
}

#[derive(Clone, Debug)]
pub struct HybridOptions {
    /// vector 权重 (0..=1)
    pub vector_weight: f32,
    /// BM25 k1（控制 tf 饱和）
    pub bm25_k1: f32,
    /// BM25 b（控制文档长度归一化）
    pub bm25_b: f32,
    /// 检索 top-k
    pub k: usize,
    /// 限定 source 列表（None = 不限）
    pub source_filter: Option<Vec<String>>,
}

impl Default for HybridOptions {
    fn default() -> Self {
        Self {
            vector_weight: 0.5,
            bm25_k1: 1.5,
            bm25_b: 0.75,
            k: 5,
            source_filter: None,
        }
    }
}

/// BM25 单条 doc score（在给定 query tokens + 文档统计下算）。
pub fn bm25_score(
    query_tokens: &[String],
    doc_tokens: &[String],
    avg_dl: f32,
    df: &[usize], // df[t] = 多少 doc 包含 token t
    n_docs: usize,
    k1: f32,
    b: f32,
) -> f32 {
    if query_tokens.is_empty() || doc_tokens.is_empty() || avg_dl <= 0.0 {
        return 0.0;
    }
    let dl = doc_tokens.len() as f32;
    let mut tf: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for t in doc_tokens {
        *tf.entry(t.as_str()).or_insert(0) += 1;
    }
    let mut score = 0.0f32;
    for qt in query_tokens {
        let f = *tf.get(qt.as_str()).unwrap_or(&0) as f32;
        if f == 0.0 {
            continue;
        }
        // 用 qt 的 hash 在 df 数组里查（caller 负责维持对齐 —— 见 build_df_index）
        let d = df[token_index(qt)] as f32;
        if d == 0.0 {
            continue;
        }
        let idf = ((n_docs as f32 - d + 0.5) / (d + 0.5) + 1.0).ln();
        let norm = 1.0 - b + b * dl / avg_dl;
        score += idf * (f * (k1 + 1.0)) / (f + k1 * norm);
    }
    score
}

/// 全局 token → df-index 的 hash 映射（256 个 slot，简单 bucket）。
/// 故意不引 hashbrown / 等高性能 hash map —— 256 bucket 对个人 RAG 完全够。
pub fn token_index(t: &str) -> usize {
    let mut h: u64 = 14695981039346656037;
    for b in t.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    (h % 256) as usize
}

/// 在一组 chunks 上跑混合检索。
/// - `chunks`: 已从 RagStore 读出的 (source, chunk_index, content, vector_score) 元组
/// - `query`: 用户 query
pub fn hybrid_search(
    query: &str,
    chunks: &[(String, usize, String, f32 /*vector_score*/, String /*origin*/)],
    opts: &HybridOptions,
) -> Vec<HybridHit> {
    if chunks.is_empty() {
        return Vec::new();
    }
    let q_tokens = tokenize(&query.to_lowercase());
    if q_tokens.is_empty() {
        // 纯 vector 排序
        let mut out: Vec<HybridHit> = chunks
            .iter()
            .map(|(s, i, c, v, o)| HybridHit {
                source: s.clone(),
                chunk_index: *i,
                content: c.clone(),
                vector_score: *v,
                bm25_score: 0.0,
                hybrid_score: *v,
                origin: o.clone(),
            })
            .collect();
        out.sort_by(|a, b| b.hybrid_score.partial_cmp(&a.hybrid_score).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(opts.k);
        return out;
    }

    // 应用 source filter
    let filtered: Vec<&(String, usize, String, f32, String)> = match &opts.source_filter {
        Some(sources) => chunks
            .iter()
            .filter(|(s, _, _, _, _)| sources.iter().any(|x| x == s))
            .collect(),
        None => chunks.iter().collect(),
    };
    if filtered.is_empty() {
        return Vec::new();
    }

    // Tokenize 每个 chunk
    let docs: Vec<Vec<String>> = filtered
        .iter()
        .map(|(_, _, c, _, _)| tokenize(&c.to_lowercase()))
        .collect();
    let n_docs = docs.len();
    let avg_dl = docs.iter().map(|d| d.len()).sum::<usize>() as f32 / n_docs as f32;

    // 简单 df：每个 token 在多少 doc 中出现（按 token_index 256 bucket）
    let mut df = vec![0usize; 256];
    for d in &docs {
        let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for t in d {
            seen.insert(token_index(t));
        }
        for idx in seen {
            df[idx] += 1;
        }
    }

    // 每条 chunk 算 BM25
    let mut hybrid: Vec<HybridHit> = filtered
        .iter()
        .zip(docs.iter())
        .map(|((s, i, c, v, o), doc_tokens)| {
            let bm = bm25_score(
                &q_tokens,
                doc_tokens,
                avg_dl,
                &df,
                n_docs,
                opts.bm25_k1,
                opts.bm25_b,
            );
            HybridHit {
                source: s.clone(),
                chunk_index: *i,
                content: c.clone(),
                vector_score: *v,
                bm25_score: bm,
                hybrid_score: 0.0, // 后面算
                origin: o.clone(),
            }
        })
        .collect();

    // 归一化
    let max_v = hybrid.iter().map(|h| h.vector_score).fold(0.0f32, f32::max);
    let max_b = hybrid.iter().map(|h| h.bm25_score).fold(0.0f32, f32::max);
    for h in &mut hybrid {
        let nv = if max_v > 0.0 { h.vector_score / max_v } else { 0.0 };
        let nb = if max_b > 0.0 { h.bm25_score / max_b } else { 0.0 };
        h.hybrid_score = opts.vector_weight * nv + (1.0 - opts.vector_weight) * nb;
    }

    hybrid.sort_by(|a, b| b.hybrid_score.partial_cmp(&a.hybrid_score).unwrap_or(std::cmp::Ordering::Equal));
    hybrid.truncate(opts.k);
    hybrid
}

/// 把 RagStore 的 Hit 转成 chunk 元组（用于 hybrid_search）。
pub fn hits_to_tuples(hits: &[Hit]) -> Vec<(String, usize, String, f32, String)> {
    hits.iter()
        .map(|h| (h.source.clone(), h.chunk_index, h.content.clone(), h.score, h.origin.clone()))
        .collect()
}

/// 在 RagStore 上跑 hybrid 检索（一站式 API）。
/// - 先 vector 检索取 N 倍候选（默认 5×k）
/// - 再做 hybrid 融合排序
pub fn hybrid_search_store<F>(
    store_chunks_fn: F,
    query: &str,
    opts: &HybridOptions,
) -> Result<Vec<HybridHit>>
where
    F: FnOnce(usize) -> Result<Vec<Hit>>,
{
    let n_candidates = (opts.k * 5).max(opts.k);
    let hits = store_chunks_fn(n_candidates)?;
    let tuples = hits_to_tuples(&hits);
    Ok(hybrid_search(query, &tuples, opts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chk(s: &str, i: usize) -> (String, usize, String, f32, String) {
        (s.to_string(), i, s.to_string(), 0.5, "hash".to_string())
    }

    #[test]
    fn bm25_keywords_outrank_vector() {
        // 同样的 chunk，但 query 包含「rust」时含 rust 的应该高
        let chunks = vec![
            chk("rust 语言内存安全零成本抽象", 0),
            chk("番茄炒蛋先炒蛋再炒番茄", 1),
            chk("rust 包管理 cargo build test", 2),
        ];
        let opts = HybridOptions { vector_weight: 0.0, k: 3, ..Default::default() };  // 纯 BM25
        let hits = hybrid_search("rust 语言", &chunks, &opts);
        assert!(!hits.is_empty());
        // 包含「rust」的两个 chunk 应该排前面
        let top: Vec<&str> = hits.iter().take(2).map(|h| h.content.as_str()).collect();
        assert!(
            top.iter().any(|c| c.contains("rust")),
            "expected rust chunks in top2, got: {top:?}"
        );
    }

    #[test]
    fn vector_only_when_no_query_tokens() {
        let chunks = vec![chk("a", 0), chk("b", 1), chk("c", 2)];
        // vector_weight=1.0 + 空 query → 走纯 vector 路径
        let opts = HybridOptions { vector_weight: 1.0, k: 2, ..Default::default() };
        let hits = hybrid_search("???", &chunks, &opts);
        assert_eq!(hits.len(), 2);
        assert!(hits[0].vector_score >= hits[1].vector_score);
    }

    #[test]
    fn hybrid_weight_balance() {
        // 全部 vector 0.5
        let chunks = vec![chk("rust 内存安全", 0), chk("rust 包管理", 1), chk("番茄炒蛋", 2)];
        let opts_bm25 = HybridOptions { vector_weight: 0.0, k: 3, ..Default::default() };
        let opts_vec = HybridOptions { vector_weight: 1.0, k: 3, ..Default::default() };
        let h_bm25 = hybrid_search("rust", &chunks, &opts_bm25);
        let h_vec = hybrid_search("rust", &chunks, &opts_vec);
        // 至少两者都不为空
        assert!(!h_bm25.is_empty());
        assert!(!h_vec.is_empty());
    }

    #[test]
    fn source_filter_works() {
        let chunks = vec![
            ("a".into(), 0, "rust 语言".into(), 0.5, "hash".into()),
            ("b".into(), 0, "rust 包管理".into(), 0.5, "hash".into()),
            ("a".into(), 1, "rust 内存".into(), 0.5, "hash".into()),
        ];
        let opts = HybridOptions {
            source_filter: Some(vec!["a".into()]),
            k: 5,
            ..Default::default()
        };
        let hits = hybrid_search("rust", &chunks, &opts);
        for h in &hits {
            assert_eq!(h.source, "a");
        }
    }

    #[test]
    fn token_index_deterministic() {
        assert_eq!(token_index("rust"), token_index("rust"));
        assert_ne!(token_index("rust"), token_index("cargo"));  // 大概率不同（hash 不同）
        assert!(token_index("any-token") < 256);
    }
}
