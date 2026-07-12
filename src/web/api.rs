//! API handlers（status / skills / sandbox / mcp / rag / command）。

use crate::repl::context::AppContext;
use crate::web::auth;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};
use std::sync::Arc;

/// Shared state for handlers.
#[derive(Clone)]
pub struct ApiState {
    pub ctx: Arc<AppContext>,
    pub token: String,
    pub auth_enabled: bool,
}

/// 鉴权失败返回 true（表示要 401）
fn unauthorized(headers: &HeaderMap, state: &ApiState) -> bool {
    if !state.auth_enabled {
        return false;
    }
    !auth::check_bearer(headers, &state.token)
}

/// `GET /api/status` — 综合状态
pub async fn get_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Json<Value> {
    if unauthorized(&headers, &state) {
        return Json(json!({ "error": "unauthorized" }));
    }
    let ctx = state.ctx.clone();
    // 收集所有需要的数据到一个 `Value`，让 lock guard 早 drop，future 变 Send
    let v = tokio::task::spawn_blocking(move || {
        let models = ctx.models.lock().unwrap();
        let settings = ctx.settings.lock().unwrap();
        let sandbox_enabled = ctx.sandbox.lock().unwrap().enabled;
        let rag_count = ctx.rag.count().unwrap_or(0);
        let skills = ctx.skills.all();
        let cwd = ctx.cwd.lock().unwrap().display().to_string();
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "provider": models.settings.default_provider,
            "backup_provider": models.settings.backup_provider,
            "lang": settings.lang,
            "autonomous": settings.autonomous,
            "cwd": cwd,
            "sandbox": { "enabled": sandbox_enabled },
            "rag":    { "chunks": rag_count },
            "skills": { "count": skills.len() },
        })
    })
    .await
    .unwrap_or_else(|e| json!({ "error": format!("join error: {e}") }));
    // 单独 await MCP（async）
    let mcp_servers = state.ctx.mcp.list_servers().await;
    let mcp_tools_count: usize = mcp_servers.iter().map(|s| s.tools.len()).sum();
    let mut v = v;
    v["mcp"] = json!({ "servers": mcp_servers.len(), "tools": mcp_tools_count });
    Json(v)
}

/// `GET /api/skills` — skills 列表
pub async fn get_skills(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Json<Value> {
    if unauthorized(&headers, &state) {
        return Json(json!({ "error": "unauthorized" }));
    }
    let skills = state.ctx.skills.all();
    let items: Vec<Value> = skills
        .iter()
        .map(|s| {
            json!({
                "name": s.frontmatter.name,
                "description": s.frontmatter.description,
                "triggers": s.frontmatter.triggers,
                "allowed_tools": s.frontmatter.allowed_tools,
                "max_steps": s.frontmatter.max_steps,
            })
        })
        .collect();
    Json(json!({ "skills": items, "count": items.len() }))
}

/// `GET /api/sandbox` — sandbox 策略
pub async fn get_sandbox(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Json<Value> {
    if unauthorized(&headers, &state) {
        return Json(json!({ "error": "unauthorized" }));
    }
    let p = state.ctx.sandbox.lock().unwrap().clone();
    Json(serde_json::to_value(&p).unwrap_or(json!({})))
}

/// `GET /api/mcp` — MCP servers
pub async fn get_mcp(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Json<Value> {
    if unauthorized(&headers, &state) {
        return Json(json!({ "error": "unauthorized" }));
    }
    let servers = state.ctx.mcp.list_servers().await;
    let items: Vec<Value> = servers
        .into_iter()
        .map(|s| {
            json!({
                "name": s.name,
                "url": s.url,
                "status": format!("{:?}", s.status).to_lowercase(),
                "tools_count": s.tools.len(),
                "tools": s.tools.iter().map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                    })
                }).collect::<Vec<_>>(),
                "last_error": s.last_error,
            })
        })
        .collect();
    // Round 15a: 加 resources + prompts 摘要
    let resources = state.ctx.mcp.all_resources().await;
    let resources_json: Vec<Value> = resources
        .into_iter()
        .map(|(server, r)| {
            json!({
                "server": server,
                "uri": r.uri,
                "name": r.name,
                "description": r.description,
                "mimeType": r.mime_type,
            })
        })
        .collect();
    let prompts = state.ctx.mcp.all_prompts().await;
    let prompts_json: Vec<Value> = prompts
        .into_iter()
        .map(|(server, p)| {
            json!({
                "server": server,
                "name": p.name,
                "description": p.description,
                "arguments_count": p.arguments.len(),
            })
        })
        .collect();
    Json(json!({
        "servers": items,
        "count": items.len(),
        "resources": resources_json,
        "prompts": prompts_json,
    }))
}

/// `GET /api/rag` — RAG stats + sources
pub async fn get_rag(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Json<Value> {
    if unauthorized(&headers, &state) {
        return Json(json!({ "error": "unauthorized" }));
    }
    let count = state.ctx.rag.count().unwrap_or(0);
    let sources = state.ctx.rag.list().unwrap_or_default();
    let auto = state.ctx.rag.auto();
    let items: Vec<Value> = sources
        .into_iter()
        .map(|s| {
            json!({
                "name": s.name,
                "count": s.count,
                "last_updated": s.last_updated,
            })
        })
        .collect();
    Json(json!({
        "chunks": count,
        "sources": items,
        "auto_recall": auto,
    }))
}

/// `POST /api/command` — 跑一个 REPL 命令，返回 JSON。
pub async fn post_command(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    if unauthorized(&headers, &state) {
        return Json(json!({ "ok": false, "error": "unauthorized" }));
    }
    let cmd = body.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
    if cmd.is_empty() {
        return Json(json!({ "ok": false, "error": "missing `cmd`" }));
    }
    let line = if cmd.starts_with('/') {
        cmd.to_string()
    } else {
        format!("/{cmd}")
    };
    let result = crate::repl::command::dispatch(&line, &state.ctx).await;
    match result {
        Ok(_outcome) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": format!("{e:#}") })),
    }
}
