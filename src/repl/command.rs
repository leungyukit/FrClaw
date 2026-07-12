//! REPL 命令路由器 + 文本输入派发。

use crate::agent::loop_runner::{run_agent_step_loop, AgentRunOptions, AgentStep};
use crate::memory::compressor::maybe_compact;
use crate::memory::project as project_memory;
use crate::repl::commands;
use crate::repl::context::AppContext;
use crate::tools::plan::all_tool_definitions;
use crate::ui::colors;
use anyhow::Result;
use std::sync::Arc;

/// 内置命令处理结果。
pub enum CmdOutcome {
    Continue,
    Exit,
}

/// 路由一条 `/xxx ...` 命令到具体实现。
pub async fn dispatch(line: &str, ctx: &AppContext) -> Result<CmdOutcome> {
    let line = line.trim();
    let mut parts = line.split_whitespace();
    let head = match parts.next() {
        Some(h) => h,
        None => return Ok(CmdOutcome::Continue),
    };

    let cmd = head.trim_start_matches('/');
    let rest_args: Vec<&str> = parts.collect();
    let joined = rest_args.join(" ");

    match cmd {
        "" => Ok(CmdOutcome::Continue),
        "help" | "?" => commands::help(ctx).await,
        "exit" | "quit" | "q" => commands::exit(ctx).await,
        "bye" => commands::exit(ctx).await,
        "banner" => commands::banner(ctx).await,
        "new" => commands::new_session(ctx, &rest_args).await,
        "save" => commands::save(ctx, &rest_args).await,
        "load" => commands::load(ctx, &rest_args).await,
        "list_sessions" | "ls_s" => commands::list_sessions(ctx).await,
        "see" | "messages" => commands::see(ctx, &rest_args).await,
        "model" | "use" => commands::model(ctx, &rest_args).await,
        "providers" => commands::providers(ctx).await,
        "key" => commands::key(ctx, &rest_args).await,
        "lang" => commands::lang(ctx, &rest_args).await,
        "limit" => commands::limit(ctx, &rest_args).await,
        "autonomous" => commands::autonomous(ctx, &rest_args).await,
        "mode" => commands::mode(ctx, &rest_args).await,
        "memory" | "mem" => commands::project_memory_cmd(ctx, &rest_args).await,
        "memory_topics" | "topics" => commands::memory_topics(ctx, &rest_args).await,
        "memory_evolve" | "evolve" => commands::memory_evolve(ctx, &rest_args).await,
        "compact" => commands::compact(ctx).await,
        "hooks" => commands::hooks_cmd(ctx, &rest_args).await,
        "skill" | "skills" => commands::skill_cmd(ctx, &rest_args).await,
        "rag" => commands::rag_cmd(ctx, &rest_args).await,
        "mcp" => commands::mcp_cmd(ctx, &rest_args).await,
        "worktree" | "wt" => commands::worktree_cmd(ctx, &rest_args).await,
        "multi_edit" | "me" => commands::multi_edit_cmd(ctx, &joined).await,
        "sandbox" | "sb" => commands::sandbox_cmd(ctx, &rest_args).await,
        "tasks" | "task" => commands::tasks_cmd(ctx, &rest_args).await,
        "cron" => commands::cron_cmd(ctx, &rest_args).await,
        "soul" => commands::soul_cmd(ctx, &rest_args).await,
        "heartbeat" | "hb" => commands::heartbeat_cmd(ctx, &rest_args).await,
        "pptx" => commands::pptx_cmd(ctx, &rest_args).await,
        "tts" => commands::tts_cmd(ctx, &rest_args).await,
        "channels" | "ch" => commands::channels_cmd(ctx, &rest_args).await,
        "timeline" | "tl" => commands::timeline_cmd(ctx, &rest_args).await,
        "voice" | "v" => commands::voice_cmd(ctx, &rest_args).await,
        "shell" | "sh" | "!" => commands::shell(ctx, &rest_args).await,
        "read" => commands::read(ctx, &rest_args).await,
        "write" => commands::write(ctx, &rest_args).await,
        "dir" | "cd" | "pwd" => commands::dir(ctx, &rest_args).await,
        "usage" => commands::usage(ctx).await,
        "version" => commands::version(ctx).await,
        "clear" => commands::clear().await,
        "doctor" => commands::doctor(ctx).await,
        other => {
            let suggestion = best_match(other);
            colors::print_error(&format!("未知命令: /{}", other.trim_start_matches('/')));
            if let Some(s) = suggestion {
                colors::print_info(&format!("你是不是想打 /{s} ?"));
            }
            Ok(CmdOutcome::Continue)
        }
    }
}

/// 用户输入「直接消息」（非 `/` 开头）：session 入栈 → 走 agent loop。
pub async fn handle_user_input(text: &str, ctx: &AppContext) -> Result<()> {
    let user_msg = text.trim().to_string();
    if user_msg.is_empty() {
        return Ok(());
    }

    // 0) Hooks: UserPromptSubmit（可阻止 / 改 prompt / 追加上下文）
    let session_name = ctx.session.lock().unwrap().name.clone();
    let up_inp = crate::hooks::events::HookInput::user_prompt_submit(
        user_msg.clone(),
        Some(session_name.clone()),
    );
    let up_out = {
        let hooks_cfg = ctx.hooks.lock().unwrap().clone();
        let out = crate::hooks::runner::dispatch_event_blocking(
            &hooks_cfg,
            crate::hooks::events::HookEvent::UserPromptSubmit,
            &up_inp,
        );
        // 即时回显 hook 写了什么 —— 让 hook 用户能直观看到「跑了」
        if let Some(tail) = &out.stderr_tail {
            eprintln!("  [UserPromptSubmit hook] {tail}");
        }
        if out.additional_context.is_some() {
            eprintln!(
                "  [UserPromptSubmit hook → additional_context 已注入 user 消息尾部]"
            );
        }
        if out.modified_prompt.is_some() {
            eprintln!("  [UserPromptSubmit hook → prompt 已被修改]");
        }
        out
    };
    if up_out.blocked {
        colors::print_error(&format!(
            "UserPromptSubmit hook 阻止：{}",
            up_out.reason.as_deref().unwrap_or("(no reason)")
        ));
        return Ok(());
    }
    let user_msg = up_out.modified_prompt.unwrap_or(user_msg);
    let additional_ctx = up_out.additional_context.clone();

    // 1) Skills 触发匹配（Round 5）—— 命中的 skill body 拼到 user 消息尾
    let matched_skills = ctx.skills.match_query(&user_msg);
    if !matched_skills.is_empty() {
        eprintln!(
            "  [skills] 触发命中 {} 条: {}",
            matched_skills.len(),
            matched_skills
                .iter()
                .map(|s| s.frontmatter.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let skills_block = if matched_skills.is_empty() {
        String::new()
    } else {
        let mut sb = String::from("\n\n[Skill 触发命中]\n");
        for s in &matched_skills {
            sb.push_str(&s.to_system_block());
            sb.push_str("\n---\n");
        }
        sb
    };

    // 1.5) RAG auto-recall（Round 7）—— 用户消息 > 4 字时自动 query 一次
    let rag_block = if ctx.rag.auto() && user_msg.chars().count() > 4 && !user_msg.starts_with('/') {
        match ctx.rag.query(&user_msg, 2) {
            Ok(hits) if !hits.is_empty() => {
                let mut rb = String::from("\n\n[RAG 召回]\n");
                for h in &hits {
                    if h.score < 0.02 {
                        // 太低质量，跳过
                        continue;
                    }
                    let preview = if h.content.chars().count() > 200 {
                        let s: String = h.content.chars().take(200).collect();
                        format!("{s}…")
                    } else {
                        h.content.clone()
                    };
                    rb.push_str(&format!(
                        "- [{} #{}] (score={:.3}) {}\n",
                        h.source, h.chunk_index, h.score, preview
                    ));
                }
                eprintln!("  [rag] auto-recall 命中 {} 条", hits.len());
                rb
            }
            _ => String::new(),
        }
    } else {
        String::new()
    };

    // 2) user 入栈；如果 hook 给了 additional_context / skill 命中/RAG 召回，都拼到消息尾
    let final_user_msg = match &additional_ctx {
        Some(ctx_text) => format!("{user_msg}\n\n[Hook 追加上下文]\n{ctx_text}{skills_block}{rag_block}"),
        None => {
            if skills_block.is_empty() && rag_block.is_empty() {
                user_msg.clone()
            } else {
                format!("{user_msg}{skills_block}{rag_block}")
            }
        }
    };
    ctx.session.lock().unwrap().push_user(final_user_msg.clone());

    // 2) 装出 messages：history 截断 + 项目记忆注入 + thinking directive
    let (messages, model_alias, max_tokens, temperature, thinking) = {
        let session = ctx.session.lock().unwrap();
        let models = ctx.models.lock().unwrap();
        let window = models.settings.history_window;
        let mut messages = session.truncated_messages(window);

        // 注入项目记忆（如果有）
        if let Some(mem) = ctx.project_memory.lock().unwrap().clone() {
            if let Some(first) = messages.first_mut() {
                if matches!(first.role, crate::llm::message::Role::System) {
                    first.content = project_memory::inject_into_system(&first.content, &mem);
                }
            }
        }
        // Round 13 ─ 注入 SOUL（持久身份）
        {
            let soul = ctx.soul.content.lock().unwrap().clone();
            if !soul.is_empty() {
                if let Some(first) = messages.first_mut() {
                    if matches!(first.role, crate::llm::message::Role::System) {
                        first.content.push_str(&soul.to_system_block());
                    }
                }
            }
        }
        // 注入 thinking 指令
        let thinking = *ctx.thinking.lock().unwrap();
        let directive = thinking.suffix_directive();
        if !directive.is_empty() {
            if let Some(first) = messages.first_mut() {
                if matches!(first.role, crate::llm::message::Role::System) {
                    first.content.push_str(directive);
                }
            }
        }

        let provider = models.get(&session.provider_alias).cloned();
        let alias = session.provider_alias.clone();
        let max = provider
            .as_ref()
            .and_then(|p| p.max_tokens)
            .unwrap_or(models.settings.max_tokens_limit);
        let temp = provider.as_ref().and_then(|p| p.temperature);
        (messages, alias, Some(max), temp, thinking)
    };

    colors::print_role(
        "user",
        &format!("[{model_alias}] {user_msg}"),
    );
    println!();

    // 3) 工具定义列表：builtin + plan + sub-agent + mcp
    let mut tools = all_tool_definitions();
    // Round 6 ─ 追加 MCP server 暴露的 tool（只列已连上的）
    let mcp_defs = ctx.mcp.tool_definitions().await;
    tools.extend(mcp_defs);

    // 4) 找到对应 provider（fallback chain 由调用方在外面）
    let (used_alias, primary) = match pick_primary(&ctx.chain, &model_alias) {
        Ok(v) => v,
        Err(e) => {
            colors::print_error(&format!("没有可用 provider: {e}"));
            return Ok(());
        }
    };
    if used_alias != model_alias {
        colors::print_info(&format!("⚠️  已自动降级到 provider `{used_alias}`"));
        ctx.session.lock().unwrap().provider_alias = used_alias.clone();
    }

    // 5) 在调 LLM 之前做一次 token 上下文压缩（安全网）
    {
        let mut s = ctx.session.lock().unwrap();
        let threshold = (model_alias.len() + 32).max(20) * 4; // 粗略估算
        maybe_compact(&mut s, 32, 16);
        let _ = threshold;
    }

    // 6) 真正跑 agent loop
    let session_name_for_hooks = ctx.session.lock().unwrap().name.clone();
    let opts = AgentRunOptions {
        messages: messages.clone(),
        tools,
        max_steps: 10,
        max_tokens,
        temperature,
        permission: ctx.permission.lock().unwrap().clone_for_agent(),
        plan_state: ctx.plan_state.lock().unwrap().clone(),
        sub_agents: (*ctx.sub_agents).clone(),
        thinking,
        hooks_cfg: ctx.hooks.lock().unwrap().clone(),
        session_name_for_hooks,
        mcp: ctx.mcp.clone(),
        rag: ctx.rag.clone(),
        worktree: ctx.worktree.clone(),
        sandbox: ctx.sandbox.clone(),
        soul: ctx.soul.clone(),
        heartbeat_tools: ctx.heartbeat_tools.clone(),
        channels: ctx.channels.clone(),
    };

    let mut final_text = String::new();
    let mut saw_stream = false;
    // Round 10 ─ 流式 markdown 渲染（行级 flush）
    let mut md_stream = crate::ui::markdown_stream::MarkdownStream::new();

    // Round 4 ─ FR_NO_LLM=1 时跳过真实 LLM 调用，但保留 user 入栈、hook 链路。
    if std::env::var("FR_NO_LLM").is_ok() {
        println!();
        let stub_ctx_note = additional_ctx
            .as_ref()
            .map(|s| format!(" (+{} chars hook ctx)", s.len()))
            .unwrap_or_default();
        crate::ui::colors::print_info("(FR_NO_LLM=1，跳过真实 LLM 调用；hook 链路已跑)");
        {
            let mut s = ctx.session.lock().unwrap();
            s.push_assistant(format!(
                "[stub] FR_NO_LLM=1 — prompt 已入栈: {user_msg}{ctx}",
                user_msg = final_user_msg.chars().take(120).collect::<String>(),
                ctx = stub_ctx_note
            ));
            let _ = s.save();
        }
        return Ok(());
    }

    {
        let provider: Arc<dyn crate::llm::provider::LlmProvider> = primary;
        let result = run_agent_step_loop(
            provider,
            opts,
            |step| match step {
                AgentStep::LlmReply { content, tool_calls, tokens } => {
                    if !content.is_empty() {
                        // Round 10 ─ 流式 markdown 渲染
                        md_stream.add(&content);
                        saw_stream = true;
                    }
                    if !tool_calls.is_empty() {
                        if saw_stream {
                            md_stream.finish();
                            println!();
                            saw_stream = false;
                        }
                        for c in tool_calls {
                            colors::print_info(&format!(
                                "  · 调用工具 `{}` ({})",
                                c.function.name,
                                short_args(&c.function.arguments)
                            ));
                        }
                    }
                    if let (Some(p), Some(c)) = tokens {
                        // 已经在 LlmReply 流式了，token 计数放后面避免打断
                        colors::print_info(&format!(
                            "  (tokens: prompt={p}, completion={c})"
                        ));
                    }
                }
                AgentStep::ToolExecuted { name, elapsed_ms, result, .. } => {
                    colors::print_info(&format!(
                        "  ✓ `{name}` 完成 ({}ms)",
                        elapsed_ms
                    ));
                    let preview = result_preview(result);
                    if !preview.is_empty() {
                        for line in preview.lines().take(4) {
                            println!("    | {line}");
                        }
                    }
                }
                AgentStep::Final { content, total_steps, prompt_tokens, completion_tokens } => {
                    final_text = content.clone();
                    colors::print_info(&format!(
                        "  (final: {total_steps} steps, prompt={}, completion={})",
                        prompt_tokens.unwrap_or(0),
                        completion_tokens.unwrap_or(0)
                    ));
                }
            },
        )
        .await?;
        if saw_stream {
            md_stream.finish();
            println!();
        }

        final_text = result.content.trim().to_string();
    }

    if final_text.is_empty() {
        // agent 没生成正文（罕见），给个回退
        final_text = "(agent 未返回文本内容)".into();
    }

    crate::ui::colors::print_assistant(&final_text);

    // 7) 写回 session + 落盘
    {
        let mut s = ctx.session.lock().unwrap();
        s.push_assistant(final_text);
        let _ = s.save();
    }
    Ok(())
}

/// 从 FallbackChain 选一个 provider 跑当前轮。
fn pick_primary(
    chain: &crate::llm::registry::FallbackChain,
    preferred: &str,
) -> Result<(String, Arc<dyn crate::llm::provider::LlmProvider>)> {
    if let Some(p) = chain.primary_for_alias(preferred) {
        return Ok((preferred.to_string(), Arc::clone(p)));
    }
    chain
        .providers()
        .first()
        .map(|(n, p)| (n.clone(), Arc::clone(p)))
        .ok_or_else(|| anyhow::anyhow!("降级链为空"))
}

fn short_args(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.len() > 80 {
        let mut r = s.chars().take(80).collect::<String>();
        r.push('…');
        r
    } else {
        s
    }
}

fn result_preview(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.len() > 400 {
        let mut r: String = s.chars().take(400).collect();
        r.push('…');
        r
    } else {
        s
    }
}

fn best_match(input: &str) -> Option<String> {
    let candidates: &[&str] = &[
        "help", "exit", "banner", "new", "save", "load", "list_sessions", "see",
        "model", "providers", "key", "lang", "limit", "autonomous", "mode",
        "shell", "read", "write", "dir", "usage", "version", "clear", "doctor",
        "memory", "compact",
    ];
    let mut best: Option<(&str, usize)> = None;
    for c in candidates {
        let score = edit_distance(input, c);
        if score <= 3 && (best.is_none() || best.unwrap().1 > score) {
            best = Some((c, score));
        }
    }
    best.map(|(c, _)| c.to_string())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}
