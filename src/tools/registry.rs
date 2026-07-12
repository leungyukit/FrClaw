//! LLM 可见工具的元信息 + dispatch 实现。

use crate::channels::ChannelManager;
use crate::llm::message::ToolDefinition;
use crate::mcp::McpManager;
use crate::sandbox::policy::SandboxPolicy;
use crate::tools::heartbeat::HeartbeatToolContext;
use crate::tools::rag::RagContext;
use crate::tools::soul::SoulContext;
use crate::tools::worktree::WorktreeContext;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// 列出 LLM 可见的所有内置工具定义。
pub fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::from_json_schema(
            "read_file",
            "读取本地文件内容（utf-8，path 必填）",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "绝对路径或相对路径" }
                },
                "required": ["path"]
            }),
        ),
        ToolDefinition::from_json_schema(
            "write_file",
            "把内容写入本地文件（覆盖；path 与 content 必填）",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        ),
        ToolDefinition::from_json_schema(
            "list_dir",
            "列出目录文件",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        ),
        ToolDefinition::from_json_schema(
            "shell",
            "在 shell 里执行一段命令并返回 stdout/stderr/exit_code",
            json!({
                "type": "object",
                "properties": {
                    "cmd": { "type": "string", "description": "shell 命令字符串" },
                    "cwd": { "type": "string", "description": "可选，工作目录" }
                },
                "required": ["cmd"]
            }),
        ),
        crate::tools::web_search::web_search_definition(),
        crate::tools::memorize::memorize_definition(),
        crate::tools::memorize::recall_definition(),
        crate::tools::rag::rag_add_definition(),
        crate::tools::rag::rag_query_definition(),
        crate::tools::rag::rag_list_definition(),
        crate::tools::rag::rag_remove_definition(),
        crate::tools::worktree::worktree_create_definition(),
        crate::tools::worktree::worktree_list_definition(),
        crate::tools::worktree::worktree_remove_definition(),
        crate::tools::worktree::worktree_status_definition(),
        crate::tools::multi_edit::multi_edit_definition(),
        crate::tools::soul::read_soul_definition(),
        crate::tools::soul::append_soul_definition(),
        crate::tools::heartbeat::heartbeat_status_definition(),
        crate::tools::heartbeat::heartbeat_now_definition(),
        crate::tools::heartbeat::heartbeat_set_definition(),
        crate::tools::channels_tool::channel_send_definition(),
        crate::tools::channels_tool::channel_broadcast_definition(),
        crate::tools::mcp_resources::mcp_list_resources_definition(),
        crate::tools::mcp_resources::mcp_read_resource_definition(),
        crate::tools::mcp_resources::mcp_list_prompts_definition(),
        crate::tools::mcp_resources::mcp_get_prompt_definition(),
    ]
}

/// 真实执行某个工具（不带授权检查——授权层在外面处理）。
pub async fn dispatch(name: &str, args: &Value) -> Result<Value> {
    dispatch_with_ctx(name, args, None, None, None, None, None, None, &ChannelManager::default()).await
}

/// 带 RAG / worktree / sandbox / soul / heartbeat / mcp / channels context 的 dispatch。
pub async fn dispatch_with_ctx(
    name: &str,
    args: &Value,
    rag: Option<&Arc<RagContext>>,
    worktree: Option<&Arc<WorktreeContext>>,
    sandbox: Option<&Arc<Mutex<SandboxPolicy>>>,
    soul: Option<&Arc<SoulContext>>,
    heartbeat: Option<&Arc<HeartbeatToolContext>>,
    mcp: Option<&McpManager>,
    channels: &ChannelManager,
) -> Result<Value> {
    let pol = sandbox.and_then(|s| s.lock().ok()).map(|g| SandboxPolicy::clone(&g));
    let pol_ref = pol.as_ref();
    match name {
        "read_file" => crate::tools::read_file_with_sandbox(args, pol_ref),
        "write_file" => crate::tools::write_file_with_sandbox(args, pol_ref),
        "list_dir" => crate::tools::list_dir_with_sandbox(args, pol_ref),
        "shell" => crate::tools::shell_with_sandbox(args, pol_ref),
        "web_search" => crate::tools::web_search::tool_web_search(args),
        "memorize" => crate::tools::memorize::tool_memorize_with_rag(args, rag.map(|c| c.as_ref())),
        "recall" => crate::tools::memorize::tool_recall(args),
        "rag_add" => match rag {
            Some(ctx) => crate::tools::rag::tool_rag_add(ctx, args),
            None => Ok(json!({ "error": "rag not initialized" })),
        },
        "rag_query" => match rag {
            Some(ctx) => crate::tools::rag::tool_rag_query(ctx, args),
            None => Ok(json!({ "error": "rag not initialized" })),
        },
        "rag_list" => match rag {
            Some(ctx) => crate::tools::rag::tool_rag_list(ctx, args),
            None => Ok(json!({ "error": "rag not initialized" })),
        },
        "rag_remove" => match rag {
            Some(ctx) => crate::tools::rag::tool_rag_remove(ctx, args),
            None => Ok(json!({ "error": "rag not initialized" })),
        },
        "worktree_create" => match worktree {
            Some(ctx) => crate::tools::worktree::tool_worktree_create(ctx, args),
            None => Ok(json!({ "error": "worktree not initialized" })),
        },
        "worktree_list" => match worktree {
            Some(ctx) => crate::tools::worktree::tool_worktree_list(ctx, args),
            None => Ok(json!({ "error": "worktree not initialized" })),
        },
        "worktree_remove" => match worktree {
            Some(ctx) => crate::tools::worktree::tool_worktree_remove(ctx, args),
            None => Ok(json!({ "error": "worktree not initialized" })),
        },
        "worktree_status" => match worktree {
            Some(ctx) => crate::tools::worktree::tool_worktree_status(ctx, args),
            None => Ok(json!({ "error": "worktree not initialized" })),
        },
        "multi_edit" => crate::tools::multi_edit::tool_multi_edit(args),
        "read_soul" => match soul {
            Some(ctx) => crate::tools::soul::tool_read_soul(ctx, args),
            None => Ok(json!({ "error": "soul not initialized" })),
        },
        "append_soul" => match soul {
            Some(ctx) => crate::tools::soul::tool_append_soul(ctx, args),
            None => Ok(json!({ "error": "soul not initialized" })),
        },
        "heartbeat_status" => match heartbeat {
            Some(ctx) => crate::tools::heartbeat::tool_heartbeat_status(ctx, args),
            None => Ok(json!({ "error": "heartbeat tools not initialized" })),
        },
        "heartbeat_now" => match heartbeat {
            Some(ctx) => crate::tools::heartbeat::tool_heartbeat_now(ctx),
            None => Ok(json!({ "error": "heartbeat tools not initialized" })),
        },
        "heartbeat_set" => match heartbeat {
            Some(ctx) => crate::tools::heartbeat::tool_heartbeat_set(ctx, args),
            None => Ok(json!({ "error": "heartbeat tools not initialized" })),
        },
        "mcp_list_resources" => match mcp {
            Some(mgr) => {
                let res = mgr.all_resources().await;
                Ok(json!({
                    "resources": res.into_iter().map(|(server, r)| json!({
                        "server": server,
                        "uri": r.uri,
                        "name": r.name,
                        "description": r.description,
                        "mimeType": r.mime_type,
                    })).collect::<Vec<_>>()
                }))
            }
            None => Ok(json!({ "error": "mcp not initialized" })),
        },
        "mcp_read_resource" => match mcp {
            Some(mgr) => {
                let server = args.get("server").and_then(|v| v.as_str()).unwrap_or("");
                let uri = args.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                match mgr.read_resource(server, uri).await {
                    Ok(r) => Ok(json!({
                        "contents": r.contents.iter().map(|c| json!({
                            "uri": c.uri().unwrap_or(""),
                            "text": c.to_text(),
                        })).collect::<Vec<_>>()
                    })),
                    Err(e) => Ok(json!({ "error": format!("{e:#}") })),
                }
            }
            None => Ok(json!({ "error": "mcp not initialized" })),
        },
        "mcp_list_prompts" => match mcp {
            Some(mgr) => {
                let res = mgr.all_prompts().await;
                Ok(json!({
                    "prompts": res.into_iter().map(|(server, p)| json!({
                        "server": server,
                        "name": p.name,
                        "description": p.description,
                        "arguments": p.arguments.iter().map(|a| json!({
                            "name": a.name,
                            "description": a.description,
                            "required": a.required,
                        })).collect::<Vec<_>>()
                    })).collect::<Vec<_>>()
                }))
            }
            None => Ok(json!({ "error": "mcp not initialized" })),
        },
        "mcp_get_prompt" => match mcp {
            Some(mgr) => {
                let server = args.get("server").and_then(|v| v.as_str()).unwrap_or("");
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let prompt_args = args.get("arguments").cloned();
                match mgr.get_prompt(server, name, prompt_args).await {
                    Ok(r) => Ok(json!({
                        "description": r.description,
                        "messages": r.messages.iter().map(|m| json!({
                            "role": m.role,
                            "text": m.to_text(),
                        })).collect::<Vec<_>>()
                    })),
                    Err(e) => Ok(json!({ "error": format!("{e:#}") })),
                }
            }
            None => Ok(json!({ "error": "mcp not initialized" })),
        },
        "channel_send" => {
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let msg = crate::tools::channels_tool::build_outbound(args);
            match channels.send(name, &msg).await {
                Ok(r) => Ok(json!({
                    "channel": r.channel,
                    "ok": r.ok,
                    "error": r.error,
                    "platform_response": r.platform_response,
                })),
                Err(e) => Ok(json!({ "error": format!("{e:#}") })),
            }
        }
        "channel_broadcast" => {
            let msg = crate::tools::channels_tool::build_outbound(args);
            let results = channels.broadcast(&msg).await;
            Ok(json!({
                "results": results.into_iter().map(|r| json!({
                    "channel": r.channel,
                    "ok": r.ok,
                    "error": r.error,
                })).collect::<Vec<_>>()
            }))
        }
        _ => Ok(json!({
            "error": format!("unknown tool `{name}`"),
        })),
    }
}

/// 注册表：dispatch 一个具名调用。
pub struct ToolRegistry;

impl ToolRegistry {
    pub fn definitions() -> Vec<ToolDefinition> {
        builtin_tool_definitions()
    }
}
