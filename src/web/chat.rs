//! SSE 流式 chat endpoint。
//!
//! `POST /api/chat` 接收 `{ "message": "..." }`，把 LLM 流式 delta 推送为 SSE event。
//! 每个 event: `event: <name>\ndata: <json>\n\n`
//! 事件类型：
//! - `delta` —— `{ "content": "..." }`
//! - `tool_call` —— `{ "name": "...", "args": ... }`
//! - `tool_result` —— `{ "name": "...", "preview": "..." }`
//! - `error` —— `{ "message": "..." }`
//! - `done` —— `{}`

use crate::agent::loop_runner::{run_agent_step_loop, AgentRunOptions, AgentStep};
use crate::repl::context::AppContext;
use crate::web::api::ApiState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_util::stream::Stream;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::{mpsc, Arc};
use std::time::Duration;

pub async fn post_chat(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, axum::http::StatusCode> {
    if state.auth_enabled && !crate::web::auth::check_bearer(&headers, &state.token) {
        return Err(axum::http::StatusCode::UNAUTHORIZED);
    }
    let message = body
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if message.is_empty() {
        return Err(axum::http::StatusCode::BAD_REQUEST);
    }
    let provider = state.ctx.chain.read().unwrap().primary().cloned();
    let stream = stream_chat(state.ctx.clone(), message, provider);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

fn stream_chat(
    ctx: Arc<AppContext>,
    message: String,
    provider: Option<Arc<dyn crate::llm::provider::LlmProvider>>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        ctx.session.lock().unwrap().push_user(message.clone());

        let mut tools = crate::tools::ToolRegistry::definitions();
        let rag_tools = crate::tools::rag::tool_definitions(&ctx.rag);
        for t in rag_tools { tools.push(t); }
        let mcp_tools = ctx.mcp.tool_definitions().await;
        for t in mcp_tools { tools.push(t); }

        let (messages, _alias, max_tokens, temperature, _thinking) = {
            let session = ctx.session.lock().unwrap();
            let models = ctx.models.lock().unwrap();
            let window = models.settings.history_window;
            let messages = session.truncated_messages(window);
            let alias = models.settings.default_provider.clone();
            let max_tokens = Some(models.settings.max_tokens_limit);
            let temperature = None;
            let thinking = *ctx.thinking.lock().unwrap();
            (messages, alias, max_tokens, temperature, thinking)
        };

        let provider = match provider {
            Some(p) => p,
            None => {
                yield Ok::<_, Infallible>(Event::default().event("error").data(
                    json!({ "message": "no primary provider configured" }).to_string()
                ));
                return;
            }
        };

        // 用 std::sync::mpsc 把 callback 里的事件 push 给 async stream
        let (tx, rx) = mpsc::channel::<String>();
        let tx_for_task = tx.clone();
        let done_tx = tx.clone();

        // 后台 task 跑 agent loop
        let ctx_clone = ctx.clone();
        let opts = AgentRunOptions {
            messages,
            tools,
            max_steps: 10,
            max_tokens,
            temperature,
            permission: ctx.permission.lock().unwrap().clone_for_agent(),
            plan_state: ctx.plan_state.lock().unwrap().clone(),
            sub_agents: (*ctx.sub_agents).clone(),
            thinking: crate::agent::ThinkingMode::ReAct,
            hooks_cfg: ctx.hooks.lock().unwrap().clone(),
            session_name_for_hooks: ctx.session.lock().unwrap().name.clone(),
            mcp: ctx.mcp.clone(),
            rag: ctx.rag.clone(),
            worktree: ctx.worktree.clone(),
            sandbox: ctx.sandbox.clone(),
            soul: ctx.soul.clone(),
            heartbeat_tools: ctx.heartbeat_tools.clone(),
            channels: ctx.channels.clone(),
        };
        let provider_clone = provider.clone();
        let task = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            let result = rt.block_on(async move {
                run_agent_step_loop(provider_clone, opts, |step| {
                    let payload = match step {
                        AgentStep::LlmReply { content, tool_calls, .. } => {
                            if !content.is_empty() {
                                Some(json!({ "type": "delta", "content": content }))
                            } else if !tool_calls.is_empty() {
                                Some(json!({
                                    "type": "tool_calls",
                                    "calls": tool_calls.iter().map(|c| json!({
                                        "name": c.function.name,
                                        "args": c.function.arguments,
                                    })).collect::<Vec<_>>()
                                }))
                            } else {
                                None
                            }
                        }
                        AgentStep::ToolExecuted { name, result, .. } => {
                            let preview = result_preview(result);
                            Some(json!({
                                "type": "tool_result",
                                "name": name,
                                "preview": preview,
                            }))
                        }
                        AgentStep::Final { .. } => None,
                    };
                    if let Some(p) = payload {
                        let _ = tx_for_task.send(p.to_string());
                    }
                }).await
            });
            match result {
                Ok(resp) => {
                    // 入栈 assistant
                    let mut s = ctx_clone.session.lock().unwrap();
                    s.push_assistant(resp.content.clone());
                    let _ = s.save();
                    let _ = done_tx.send(json!({ "type": "done" }).to_string());
                }
                Err(e) => {
                    let _ = done_tx.send(json!({ "type": "error", "message": format!("{e:#}") }).to_string());
                }
            }
        });

        // 把 std::sync::mpsc 转成 stream
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(payload) => {
                    let event_name: String = serde_json::from_str::<Value>(&payload)
                        .ok()
                        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(|s| s.to_string()))
                        .unwrap_or_else(|| "message".to_string());
                    let done = event_name == "done" || event_name == "error";
                    let name = event_name.clone();
                    yield Ok::<_, Infallible>(Event::default().event(name).data(payload));
                    if done {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // 检查 task 是否完成
                    if task.is_finished() && rx.try_recv().is_err() {
                        break;
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

fn result_preview(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 200 {
        format!("{}...", &s[..200])
    } else {
        s
    }
}
