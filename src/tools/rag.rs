//! RAG 工具（LLM 可见）。
//!
//! 提供 4 个工具：
//! - `rag_add` —— 把一段文本入库
//! - `rag_query` —— 按 query 检索 top-k
//! - `rag_list` —— 列所有 source
//! - `rag_remove` —— 按 source 名删除
//!
//! 跟 `memorize` 联动：`memorize` 写完长期记忆后双写一份到 RAG
//! （`MemorizeWriteback` 通过 `RagStore::add_text_hash`）。
//!
//! 跟 `auto-recall` 联动：`handle_user_input` 在用户 prompt > 4 字时
//! 自动 query 一次，把 top-2 chunks 拼成 additional_context。

use crate::llm::message::ToolDefinition;
use crate::rag::RagStore;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct RagContext {
    pub store: Arc<std::sync::Mutex<RagStore>>,
    /// auto-recall 开关（运行时切换）
    pub auto_enabled: Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for RagContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RagContext")
            .field("auto", &self.auto())
            .field("count", &self.count().unwrap_or(0))
            .finish()
    }
}

impl RagContext {
    pub fn new(store: RagStore) -> Self {
        // 默认开启 auto-recall（user prompt > 4 字自动 query top-2 拼成 additional_context）。
        // 想要安静的话 `/rag auto off`。
        let auto = std::env::var("FR_RAG_AUTO")
            .map(|v| !matches!(v.as_str(), "0" | "false" | "no" | "off"))
            .unwrap_or(true);
        Self {
            store: Arc::new(std::sync::Mutex::new(store)),
            auto_enabled: Arc::new(std::sync::atomic::AtomicBool::new(auto)),
        }
    }

    /// 调用 `add_text_hash` 写入（hash embedder，无需 provider）。
    pub fn add(&self, source: &str, text: &str, metadata: Option<&str>) -> Result<usize> {
        let s = self.store.lock().map_err(|e| anyhow::anyhow!("rag store lock: {e}"))?;
        s.add_text_hash(source, text, metadata)
    }

    /// 调用 `query_hash` 检索。
    pub fn query(&self, query: &str, k: usize) -> Result<Vec<crate::rag::Hit>> {
        let s = self.store.lock().map_err(|e| anyhow::anyhow!("rag store lock: {e}"))?;
        s.query_hash(query, k)
    }

    pub fn list(&self) -> Result<Vec<crate::rag::SourceInfo>> {
        let s = self.store.lock().map_err(|e| anyhow::anyhow!("rag store lock: {e}"))?;
        s.list_sources()
    }

    pub fn remove(&self, name: &str) -> Result<usize> {
        let s = self.store.lock().map_err(|e| anyhow::anyhow!("rag store lock: {e}"))?;
        s.remove_source(name)
    }

    pub fn get_source(&self, name: &str) -> Result<Vec<(usize, String)>> {
        let s = self.store.lock().map_err(|e| anyhow::anyhow!("rag store lock: {e}"))?;
        s.get_source(name)
    }

    pub fn count(&self) -> Result<usize> {
        let s = self.store.lock().map_err(|e| anyhow::anyhow!("rag store lock: {e}"))?;
        s.count()
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        // 短期持有锁拿 path
        match self.store.lock() {
            Ok(s) => s.db_path().to_path_buf(),
            Err(_) => std::path::PathBuf::from("<rag store poisoned>"),
        }
    }

    pub fn set_auto(&self, on: bool) {
        self.auto_enabled.store(on, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn auto(&self) -> bool {
        self.auto_enabled.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// ─── Tool definitions ─────────────────────────────────────────────

pub fn rag_add_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "rag_add",
        "把一段文本（笔记 / 文章 / 邮件）写入个人 RAG 知识库。会自动分块 + embedding + 入 sqlite。",
        json!({
            "type": "object",
            "properties": {
                "source": { "type": "string", "description": "来源标识（文件名 / 主题）" },
                "content": { "type": "string", "description": "要入库的文本" },
                "metadata": { "type": "string", "description": "可选的 JSON 字符串 metadata" }
            },
            "required": ["source", "content"]
        }),
    )
}

pub fn rag_query_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "rag_query",
        "按 query 文本从 RAG 知识库检索 top-k 最相似的 chunk。默认走 hybrid 模式（vector + BM25 融合）。",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "k": { "type": "integer", "description": "返回数量，默认 5" },
                "hybrid": { "type": "boolean", "description": "是否走 hybrid（默认 true）" },
                "vector_weight": { "type": "number", "description": "vector 权重 0..=1，默认 0.5（hybrid 模式生效）" },
                "source": { "type": "string", "description": "可选，限定只搜某个 source" }
            },
            "required": ["query"]
        }),
    )
}

pub fn rag_list_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "rag_list",
        "列出 RAG 知识库里所有 source 及其 chunk 数。",
        json!({ "type": "object", "properties": {} }),
    )
}

pub fn rag_remove_definition() -> ToolDefinition {
    ToolDefinition::from_json_schema(
        "rag_remove",
        "按 source 名删除 RAG 知识库里的全部 chunk。",
        json!({
            "type": "object",
            "properties": {
                "source": { "type": "string" }
            },
            "required": ["source"]
        }),
    )
}

/// 给 LLM 一次返回 4 个 RAG 工具定义。
pub fn tool_definitions(_ctx: &RagContext) -> Vec<ToolDefinition> {
    vec![
        rag_add_definition(),
        rag_query_definition(),
        rag_list_definition(),
        rag_remove_definition(),
    ]
}

// ─── Tool dispatch（context 注入式） ──────────────────────────────

pub fn tool_rag_add(ctx: &RagContext, args: &Value) -> Result<Value> {
    let source = args
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("rag_add: missing `source`"))?;
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("rag_add: missing `content`"))?;
    let metadata = args.get("metadata").and_then(|v| v.as_str());
    let n = ctx.add(source, content, metadata)?;
    Ok(json!({ "source": source, "chunks_added": n }))
}

pub fn tool_rag_query(ctx: &RagContext, args: &Value) -> Result<Value> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("rag_query: missing `query`"))?;
    let k = args.get("k").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let use_hybrid = args
        .get("hybrid")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);  // Round 15b 默认开
    let vector_weight = args
        .get("vector_weight")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5) as f32;
    let source_filter = args.get("source").and_then(|v| v.as_str()).map(|s| vec![s.to_string()]);

    if use_hybrid {
        // Round 15b ─ 混合检索：vector + BM25 加权融合
        let n_candidates = (k * 5).max(k);
        let raw_hits = ctx.query(query, n_candidates)?;
        let tuples = crate::rag::hybrid::hits_to_tuples(&raw_hits);
        let opts = crate::rag::hybrid::HybridOptions {
            vector_weight,
            source_filter,
            k,
            ..Default::default()
        };
        let hits = crate::rag::hybrid::hybrid_search(query, &tuples, &opts);
        let out: Vec<Value> = hits
            .into_iter()
            .map(|h| {
                json!({
                    "source": h.source,
                    "chunk_index": h.chunk_index,
                    "hybrid_score": h.hybrid_score,
                    "vector_score": h.vector_score,
                    "bm25_score": h.bm25_score,
                    "origin": h.origin,
                    "content": h.content,
                })
            })
            .collect();
        Ok(json!({
            "query": query,
            "k": k,
            "mode": "hybrid",
            "vector_weight": vector_weight,
            "hits": out
        }))
    } else {
        let hits = ctx.query(query, k)?;
        let out: Vec<Value> = hits
            .into_iter()
            .map(|h| {
                json!({
                    "source": h.source,
                    "chunk_index": h.chunk_index,
                    "score": h.score,
                    "origin": h.origin,
                    "content": h.content,
                })
            })
            .collect();
        Ok(json!({ "query": query, "k": k, "mode": "vector", "hits": out }))
    }
}

pub fn tool_rag_list(ctx: &RagContext, _args: &Value) -> Result<Value> {
    let sources = ctx.list()?;
    let items: Vec<Value> = sources
        .into_iter()
        .map(|s| {
            json!({
                "source": s.name,
                "count": s.count,
                "last_updated": s.last_updated,
            })
        })
        .collect();
    Ok(json!({ "sources": items, "total_sources": items.len() }))
}

pub fn tool_rag_remove(ctx: &RagContext, args: &Value) -> Result<Value> {
    let source = args
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("rag_remove: missing `source`"))?;
    let n = ctx.remove(source)?;
    Ok(json!({ "source": source, "chunks_removed": n }))
}
