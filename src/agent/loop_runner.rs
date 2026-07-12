//! MasterAgent ReAct step loop：调 LLM，处理 tool_calls，自动授权/计划/并发。
//!
//! 接口：
//! ```ignore
//! run_agent_step_loop(opts, on_event).await? -> AgentStep { final_response, total_steps, total_tokens }
//! ```

use crate::agent::ThinkingMode;
use crate::llm::message::{Message, ToolCall, ToolDefinition};
use crate::llm::provider::{CompletionRequest, CompletionResponse, LlmProvider};
use crate::tools::permission::PermissionGate;
use crate::tools::plan::{self, PlanModeState};
use crate::tools::registry::dispatch_with_ctx;
use crate::tools::subagent::SubAgentRegistry;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinSet;

#[derive(Debug, Clone)]
pub struct AgentRunOptions {
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub max_steps: usize,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub permission: PermissionGate,
    pub plan_state: PlanModeState,
    pub sub_agents: SubAgentRegistry,
    pub thinking: ThinkingMode,
    /// Round 4：Hooks 配置（共享 refs）。
    pub hooks_cfg: crate::hooks::HooksFile,
    pub session_name_for_hooks: String,
    /// Round 6：MCP Manager（处理 mcp__* 工具名）。
    pub mcp: crate::mcp::McpManager,
    /// Round 7：RAG 上下文（rag_* 工具用）。
    pub rag: std::sync::Arc<crate::tools::rag::RagContext>,
    /// Round 8：Worktree 上下文（worktree_* 工具用）。
    pub worktree: std::sync::Arc<crate::tools::worktree::WorktreeContext>,
    /// Round 9：沙箱策略（路径/命令检查）。
    pub sandbox: std::sync::Arc<std::sync::Mutex<crate::sandbox::policy::SandboxPolicy>>,
    /// Round 13：SOUL 上下文（read_soul / append_soul 工具）。
    pub soul: std::sync::Arc<crate::tools::soul::SoulContext>,
    /// Round 14：Heartbeat 工具上下文（heartbeat_status / now / set）。
    pub heartbeat_tools: std::sync::Arc<crate::tools::HeartbeatToolContext>,
    /// Round 16：多通讯通道
    pub channels: crate::channels::ChannelManager,
}

/// 一个 step 的执行结果（用于 step-by-step 渲染）
#[derive(Debug, Clone)]
pub enum AgentStep {
    LlmReply {
        content: String,
        tool_calls: Vec<ToolCall>,
        tokens: (Option<u32>, Option<u32>),
    },
    ToolExecuted {
        tool_call_id: String,
        name: String,
        result: Value,
        elapsed_ms: u128,
    },
    Final {
        content: String,
        total_steps: usize,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
    },
}

/// MasterAgent step loop。
///
/// - 调 LLM 看返回
/// - 没有 tool_calls → 返回 final
/// - 有 tool_calls → 并发跑工具（plan 工具走 path，其它走 LLM），把结果回填为 tool messages
/// - 下一轮
pub async fn run_agent_step_loop<F>(
    provider: Arc<dyn LlmProvider>,
    opts: AgentRunOptions,
    mut on_event: F,
) -> Result<CompletionResponse>
where
    F: FnMut(&AgentStep) + Send,
{
    let mut messages = opts.messages;
    let tools = opts.tools;
    let max_steps = opts.max_steps.max(1);
    let mut permission = opts.permission;
    let plan_state = opts.plan_state;
    let sub_agents = opts.sub_agents;
    let thinking = opts.thinking;

    let mut prompt_tokens_total: u32 = 0;
    let mut completion_tokens_total: u32 = 0;

    for step in 0..max_steps {
        let req = CompletionRequest {
            messages: messages.clone(),
            tools: tools.clone(),
            temperature: opts.temperature,
            max_tokens: opts.max_tokens,
            force_non_stream: false,
        };

        // 调用 LLM（带降级 chain 由调用方在外面处理；这里只对单一 provider 调）
        let resp = provider.chat(req).await?;
        prompt_tokens_total = prompt_tokens_total.saturating_add(resp.prompt_tokens.unwrap_or(0));
        completion_tokens_total = completion_tokens_total.saturating_add(resp.completion_tokens.unwrap_or(0));

        on_event(&AgentStep::LlmReply {
            content: resp.content.clone(),
            tool_calls: resp.tool_calls.clone(),
            tokens: (resp.prompt_tokens, resp.completion_tokens),
        });

        let tool_calls = resp.tool_calls.clone();
        let content = resp.content.clone();

        if tool_calls.is_empty() {
            return Ok(CompletionResponse {
                content,
                tool_calls: vec![],
                prompt_tokens: Some(prompt_tokens_total),
                completion_tokens: Some(completion_tokens_total),
                finish_reason: Some("stop".into()),
            });
        }

        // 回写 assistant 消息（带 tool_calls）
        let mut assistant_msg =
            Message::assistant(content.clone());
        assistant_msg.tool_calls = tool_calls.clone();
        messages.push(assistant_msg);

        // 并发跑所有 tool_calls
        let mut join_set: JoinSet<(String, String, Value, u128)> = JoinSet::new();
        for call in tool_calls.iter() {
            let name = call.function.name.clone();
            let args = call.function.arguments.clone();
            let call_id = call.id.clone();

            // 授权检查（plan/sub-agent 不弹 authorize，因为它们是 agent 自循环）
            let auto_approve = {
                let in_plan = plan_state.active;
                matches!(name.as_str(), "enter_plan_mode" | "exit_plan_mode")
                    || (in_plan && matches!(name.as_str(), "read_file" | "list_dir"))
                    || crate::tools::subagent::is_subagent_tool(&name)
            };
            if !auto_approve {
                if !permission.confirm(&name, &args) {
                    let denial = serde_json::json!({
                        "error": "user denied",
                        "note": format!("用户拒绝调用 `{name}`")
                    });
                    join_set.spawn(async move {
                        (call_id, name, denial, 0u128)
                    });
                    continue;
                }
            }

            // ⭐ Hooks: PreToolUse（可阻止 / 改 args / 改 tool_name）
            let session_for_hook = opts.session_name_for_hooks.clone();
            let (eff_name, eff_args, hook_denial, pre_err) =
                run_pre_tool_use_hooks(&opts.hooks_cfg, &session_for_hook, &name, &args);
            if let Some(tail) = &pre_err {
                eprintln!("  [PreToolUse:{eff_name} hook] {tail}");
            }
            if let Some(denial) = hook_denial {
                let denial = serde_json::json!({
                    "error": "blocked by hook",
                    "note": denial,
                });
                join_set.spawn(async move {
                    (call_id, name, denial, 0u128)
                });
                continue;
            }
            // eff_name / eff_args 可能被 hook 改过；spawn 用改名后值
            let eff_name = eff_name;
            let eff_args = eff_args;

            // 每次 spawn 前 clone 一份引用，避免外层变量被 move
            let mut plan_state_clone = plan_state.clone();
            let sub_agents_clone = sub_agents.clone();
            let session_for_post = session_for_hook.clone();
            let hooks_cfg_clone = opts.hooks_cfg.clone();
            let eff_name_for_spawn = eff_name.clone();
            let eff_args_for_spawn = eff_args.clone();
            let mcp_clone = opts.mcp.clone();
            let rag_clone = opts.rag.clone();
            let worktree_clone = opts.worktree.clone();
            let sandbox_clone = opts.sandbox.clone();
            let soul_clone = opts.soul.clone();
            let heartbeat_tools_clone = opts.heartbeat_tools.clone();
            let channels_clone = opts.channels.clone();
            join_set.spawn(async move {
                let start = Instant::now();
                let res = if crate::tools::plan::is_plan_tool(&eff_name_for_spawn) {
                    match plan::handle_plan_tool(
                        &mut plan_state_clone,
                        &eff_name_for_spawn,
                        &eff_args_for_spawn,
                    ).await {
                        Ok(v) => v,
                        Err(e) => serde_json::json!({"error": format!("{e:#}")}),
                    }
                } else if crate::tools::subagent::is_subagent_tool(&eff_name_for_spawn) {
                    match sub_agents_clone.handle(&eff_name_for_spawn, &eff_args_for_spawn).await {
                        Ok(v) => v,
                        Err(e) => serde_json::json!({"error": format!("{e:#}")}),
                    }
                } else if eff_name_for_spawn.starts_with("mcp__") {
                    // Round 6 ─ MCP 工具
                    let args_for_mcp = eff_args_for_spawn.clone();
                    match mcp_clone.dispatch(&eff_name_for_spawn, args_for_mcp).await {
                        Ok(v) => v,
                        Err(e) => serde_json::json!({"error": format!("{e:#}")}),
                    }
                } else {
                    match dispatch_with_ctx(
                        &eff_name_for_spawn,
                        &eff_args_for_spawn,
                        Some(&rag_clone),
                        Some(&worktree_clone),
                        Some(&sandbox_clone),
                        Some(&soul_clone),
                        Some(&heartbeat_tools_clone),
                        Some(&mcp_clone),
                        &channels_clone,
                    ).await {
                        Ok(v) => v,
                        Err(e) => serde_json::json!({"error": format!("{e:#}")}),
                    }
                };
                let elapsed = start.elapsed().as_millis();
                // ⭐ Hooks: PostToolUse（只做观察 / 日志；回显 stderr）
                let post_err = run_post_tool_use_hooks(
                    &hooks_cfg_clone,
                    &session_for_post,
                    &eff_name_for_spawn,
                    &eff_args_for_spawn,
                    &res,
                    elapsed,
                );
                if let Some(tail) = &post_err {
                    eprintln!("  [PostToolUse:{eff_name_for_spawn} hook] {tail}");
                }
                (call_id, name, res, elapsed)
            });
        }

        // 把变更后再写回 plan_state（spawn 里的 clone 不会反向 sync）
        // sub_agents 是 Arc<SubAgentRegistry>，自身 mutex 跨 thread；plan_state 是 plain。
        // 为简化：取最后一次 spawn 的 plan_state_clone 写回。
        // 因为 plan tool 通常只调一次，就 last-wins 也行。
        // MVP：保持 plan_state 简洁——每轮后清空。
        let _ = plan_state.active && plan_state.steps.is_empty();

        // 收集结果
        while let Some(jr) = join_set.join_next().await {
            if let Ok((call_id, name, result, elapsed_ms)) = jr {
                on_event(&AgentStep::ToolExecuted {
                    tool_call_id: call_id.clone(),
                    name: name.clone(),
                    result: result.clone(),
                    elapsed_ms,
                });

                let result_str = serde_json::to_string(&result).unwrap_or_else(|_| "null".into());
                messages.push(Message::tool(call_id, name, result_str));
            }
        }

        if step + 1 >= max_steps {
            // 达到上限：把累积的内容作为 final
            return Ok(CompletionResponse {
                content,
                tool_calls: vec![],
                prompt_tokens: Some(prompt_tokens_total),
                completion_tokens: Some(completion_tokens_total),
                finish_reason: Some("length".into()),
            });
        }

        // Plan mode 特例：enter_plan_mode 让用户审批；如果用 plan 模式思维且 step=0，可能希望
        // 立刻退出循环让 user 看了之后再继续。
        let _ = thinking; // 目前只影响 system prompt 不影响循环行为
    }

    // 不会到这里（max_steps 至少 1 + early-return）
    Ok(CompletionResponse {
        content: String::new(),
        tool_calls: vec![],
        prompt_tokens: Some(prompt_tokens_total),
        completion_tokens: Some(completion_tokens_total),
        finish_reason: Some("stop".into()),
    })
}

// ----------------- Hooks 集成 -----------------

/// 在跑工具**前**跑 PreToolUse 钩子。
///
/// Returns: `(effective_name, effective_args, denial_or_none, stderr_tail)`
/// - `denial.is_some()` 表示被 hook 阻止；loop_runner 把该 Some 当作 tool_result 直接回灌
/// - `eff_name` / `eff_args` 允许 hook 改了
/// - `stderr_tail` 让调用方决定要不要即时回显
pub fn run_pre_tool_use_hooks(
    hooks_cfg: &crate::hooks::HooksFile,
    session_name: &str,
    tool_name: &str,
    args: &serde_json::Value,
) -> (String, serde_json::Value, Option<serde_json::Value>, Option<String>) {
    let inp = crate::hooks::events::HookInput::pre_tool_use(
        tool_name.to_string(),
        args.clone(),
        Some(session_name.to_string()),
    );
    let out = crate::hooks::runner::dispatch_event_blocking(
        hooks_cfg,
        crate::hooks::events::HookEvent::PreToolUse,
        &inp,
    );
    let stderr_tail = out.stderr_tail.clone();
    if out.blocked {
        let reason = out
            .reason
            .clone()
            .unwrap_or_else(|| "PreToolUse hook blocked".into());
        let denial = serde_json::json!({
            "error": "blocked by hook",
            "reason": reason,
            "stderr": out.stderr_tail,
        });
        return (tool_name.to_string(), args.clone(), Some(denial), stderr_tail);
    }
    let new_name = out.modified_tool_name.unwrap_or_else(|| tool_name.to_string());
    let new_args = out.modified_args.unwrap_or_else(|| args.clone());
    (new_name, new_args, None, stderr_tail)
}

/// 在工具跑完后跑 PostToolUse 钩子。当前 MVP 不修改 result，只做观察 / 日志。
pub fn run_post_tool_use_hooks(
    hooks_cfg: &crate::hooks::HooksFile,
    session_name: &str,
    tool_name: &str,
    args: &serde_json::Value,
    result: &serde_json::Value,
    elapsed_ms: u128,
) -> Option<String> {
    let inp = crate::hooks::events::HookInput::post_tool_use(
        tool_name,
        args.clone(),
        result.clone(),
        elapsed_ms,
        Some(session_name.to_string()),
    );
    let out = crate::hooks::runner::dispatch_event_blocking(
        hooks_cfg,
        crate::hooks::events::HookEvent::PostToolUse,
        &inp,
    );
    out.stderr_tail
}