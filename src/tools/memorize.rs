//! `memorize` / `recall` 工具：LLM 主动沉淀与召回长期记忆。
//!
//! `memorize({content, source?})` —— 把一条事实 / 偏好写进 `~/.fr_cli/memory/long-term.md`
//! `recall({query, k?})` —— 按关键词扫 long-term.md + evolution/*.md 找相关条目

use crate::memory::evolution;
use crate::tools::rag::RagContext;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::path::Path;

pub fn tool_memorize(args: &serde_json::Value) -> Result<serde_json::Value> {
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("memorize: missing `content`"))?;
    let source = args
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("conversation");
    evolution::memorize_from_conversation(content, source)?;
    Ok(json!({
        "ok": true,
        "wrote_bytes": content.len(),
        "source": source,
        "long_term_path": evolution::long_term_path().display().to_string(),
    }))
}

/// Round 7 ─ memorize 写入 long-term.md 后，再双写一份到 RAG。
/// 用 `source=memory:<原 source>` 标识来源。
pub fn tool_memorize_with_rag(
    args: &serde_json::Value,
    rag: Option<&RagContext>,
) -> Result<serde_json::Value> {
    let mut result = tool_memorize(args)?;
    if let (Some(rag), Some(content)) = (
        rag,
        args.get("content").and_then(|v| v.as_str()),
    ) {
        let original_source = args
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("conversation");
        let rag_source = format!("memory:{original_source}");
        match rag.add(&rag_source, content, Some("from memorize")) {
            Ok(n) => {
                result["rag_chunks_added"] = json!(n);
                result["rag_source"] = json!(rag_source);
            }
            Err(_) => {
                // 静默失败，不影响 memorize 自身成功
            }
        }
    }
    Ok(result)
}

pub fn tool_recall(args: &serde_json::Value) -> Result<serde_json::Value> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("recall: missing `query`"))?;
    let k = args.get("k").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let k = k.min(20);

    let long_term = evolution::long_term_path();
    let evolution_dir = evolution::evolution_dir();

    let mut matches = Vec::new();

    // 扫 long-term.md
    if long_term.exists() {
        if let Ok(text) = std::fs::read_to_string(&long_term) {
            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&query.to_lowercase()) {
                    matches.push(json!({
                        "source": "long-term",
                        "line": i + 1,
                        "excerpt": line.chars().take(200).collect::<String>(),
                    }));
                    if matches.len() >= k {
                        break;
                    }
                }
            }
        }
    }

    // 扫 evolution/*.md（粗匹配）
    if matches.len() < k && evolution_dir.exists() {
        let entries = std::fs::read_dir(&evolution_dir)?;
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("md")
            })
            .collect();
        files.sort();
        for f in files {
            if matches.len() >= k {
                break;
            }
            let text = match std::fs::read_to_string(&f) {
                Ok(t) => t,
                Err(_) => continue,
            };
            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&query.to_lowercase()) {
                    matches.push(json!({
                        "source": format!("evolution/{}", f.file_name().and_then(|s| s.to_str()).unwrap_or("?")),
                        "line": i + 1,
                        "excerpt": line.chars().take(200).collect::<String>(),
                    }));
                    if matches.len() >= k {
                        break;
                    }
                }
            }
            let _ = Path::new(&f);
        }
    }

    Ok(json!({
        "query": query,
        "matches": matches,
        "scanned": [evolution::long_term_path(), evolution::evolution_dir()],
    }))
}

pub fn memorize_definition() -> crate::llm::message::ToolDefinition {
    use crate::llm::message::ToolDefinition;
    ToolDefinition::from_json_schema(
        "memorize",
        "把一条事实 / 偏好 / 用户 stable 信息写进长期记忆 (long-term.md)。\
         下次启动会自动注入到 system prompt。建议在用户表达稳定偏好/事实时调用。",
        json!({
            "type": "object",
            "properties": {
                "content": { "type": "string", "description": "要记住的事实/偏好" },
                "source": { "type": "string", "description": "来源标签（默认 conversation）", "default": "conversation" }
            },
            "required": ["content"]
        }),
    )
}

pub fn recall_definition() -> crate::llm::message::ToolDefinition {
    use crate::llm::message::ToolDefinition;
    ToolDefinition::from_json_schema(
        "recall",
        "按关键词召回长期记忆（扫 long-term.md + evolution/*.md）。匹配行号 + 摘录返回。",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "k": { "type": "integer", "default": 5, "maximum": 20 }
            },
            "required": ["query"]
        }),
    )
}
