//! 应用 bootstrap：加载配置、构造 ChatSession 与 FallbackChain。

use crate::cli::args::Args;
use crate::config::models;
use crate::config::settings;
use crate::llm::registry::FallbackChain;
use crate::repl::context::AppContext;
use crate::repl::runner::ReplConfig;
use crate::session::chat::ChatSession;
use anyhow::Result;
use std::path::PathBuf;

pub fn bootstrap(args: &Args) -> Result<std::sync::Arc<AppContext>> {
    let mut models = models::load_or_init()?;
    let settings = settings::load_or_default()?;

    if let Some(alias) = args.model.as_deref() {
        if models.providers.contains_key(alias) {
            if let Some(p) = models.get_mut(alias) {
                p.is_default = true;
            }
        } else {
            eprintln!("⚠️  不存在 provider `{alias}`，忽略 --model");
        }
    }

    let cwd: PathBuf = match &args.cwd {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };

    let default_alias = models.default_provider_name().unwrap_or_default();
    if default_alias.is_empty() {
        eprintln!("⚠️  未配置任何 LLM provider；请先编辑 ~/.fr_cli/models.yaml 或执行 /model <alias> 切换。");
    }
    let session = ChatSession::new("default", &models, &settings, default_alias.clone());
    let chain = FallbackChain::from_models(&models)?;

    // 启动期自动发现项目记忆
    let project_memory_value = crate::memory::project::discover(&cwd).ok().flatten();
    // 启动期读取长期记忆 snippet（如果存在）
    let long_term_snippet = crate::memory::evolution::load_long_term_snippet();

    let ctx = std::sync::Arc::new(AppContext::new(models, settings, session, chain, cwd));
    if let Some(mem) = project_memory_value {
        if let Some(first) = ctx.session.lock().unwrap().messages.first_mut() {
            if matches!(
                first.role,
                crate::llm::message::Role::System
            ) {
                first.content =
                    crate::memory::project::inject_into_system(&first.content, &mem);
            }
        }
        *ctx.project_memory.lock().unwrap() = Some(mem);
    }
    if !long_term_snippet.is_empty() {
        if let Some(first) = ctx.session.lock().unwrap().messages.first_mut() {
            if matches!(first.role, crate::llm::message::Role::System) {
                if !first.content.contains("# 长期记忆") {
                    first.content.push_str("\n\n");
                    first.content.push_str(&long_term_snippet);
                }
            }
        }
    }

    // Round 4 — 跑 SessionStart hooks（REPL 启动 / one-shot 都跑一次）。
    let provider_alias_for_hook = ctx
        .session
        .lock()
        .unwrap()
        .provider_alias
        .clone();
    let session_name_for_hook = ctx.session.lock().unwrap().name.clone();
    let cwd_for_hook = ctx.cwd.lock().unwrap().display().to_string();
    let ss_inp = crate::hooks::events::HookInput::session_start(
        session_name_for_hook,
        cwd_for_hook,
        provider_alias_for_hook,
    );
    let ss_out = {
        let hooks_cfg = ctx.hooks.lock().unwrap().clone();
        crate::hooks::runner::dispatch_event_blocking(
            &hooks_cfg,
            crate::hooks::events::HookEvent::SessionStart,
            &ss_inp,
        )
    };
    // 把 SessionStart hook 写到 stderr 的 tail 立即显示（用户能感知）
    if let Some(tail) = &ss_out.stderr_tail {
        eprintln!("  [session_start hook] {tail}");
    }
    // Round 14 ─ 启动 Heartbeat 后台 tick
    ctx.heartbeat.start_background(ctx.clone());
    Ok(ctx)
}

pub async fn run_repl(args: Args, mut cfg: ReplConfig) -> Result<()> {
    cfg.history_file = crate::config::paths::data_dir()?.join("repl_history.txt");
    let ctx = bootstrap(&args)?;  // Arc<AppContext>
    // Round 6 ─ 启动期 connect 所有 auto_connect MCP server
    ctx.mcp.connect_all().await;
    // Round 14 ─ 把 Arc<AppContext> 注入 heartbeat_tools.app_ctx_factory
    *ctx.heartbeat_tools.app_ctx_factory.lock().unwrap() = Some(ctx.clone());
    // Round 11 ─ subcommand 路由
    if let Some(cmd) = &args.subcommand {
        match cmd {
            crate::cli::args::Commands::Web { port, host, no_auth, token } => {
                let web_cfg = crate::web::server::WebConfig {
                    host: host.clone(),
                    port: *port,
                    no_auth: *no_auth,
                    token: token.clone(),
                };
                return crate::web::server::run_web_server(ctx, web_cfg).await;
            }
        }
    }
    // 非 Web 路径：runner 也吃 Arc
    crate::repl::runner::run(ctx, cfg).await
}

/// 处理一次性 `-p / -f -c` 模式（one-shot 提问，不进入 REPL）。
pub async fn run_one_shot(args: &Args) -> Result<i32> {
    // Round 11 ─ subcommand 优先（web 模式直接接管）
    if let Some(cmd) = &args.subcommand {
        match cmd {
            crate::cli::args::Commands::Web { port, host, no_auth, token } => {
                let ctx = bootstrap(args)?;
                ctx.mcp.connect_all().await;
                *ctx.heartbeat_tools.app_ctx_factory.lock().unwrap() = Some(ctx.clone());
                let web_cfg = crate::web::server::WebConfig {
                    host: host.clone(),
                    port: *port,
                    no_auth: *no_auth,
                    token: token.clone(),
                };
                return crate::web::server::run_web_server(ctx, web_cfg)
                    .await
                    .map(|()| 0);
            }
        }
    }
    let ctx = bootstrap(args)?;
    ctx.mcp.connect_all().await;
    // 一次性模式 heartbeat 不需要后台跑（一次性退出）

    if ctx.chain.read().unwrap().providers().is_empty() {
        anyhow::bail!("未配置任何可用 LLM provider，请先编辑 ~/.fr_cli/models.yaml 并设置 default_provider");
    }

    let prompt = match (args.prompt.clone(), args.file.clone()) {
        (Some(p), _) => p,
        (None, Some(path)) => std::fs::read_to_string(&path)?,
        (None, None) => {
            anyhow::bail!("请提供 --prompt <TEXT> 或 --file <FILE>");
        }
    };

    if prompt.trim().is_empty() {
        anyhow::bail!("prompt 不能为空");
    }

    ctx.session.lock().unwrap().push_user(prompt.clone());

    let (messages, model_alias, max_tokens, temperature) = {
        let session = ctx.session.lock().unwrap();
        let models = ctx.models.lock().unwrap();
        let window = models.settings.history_window;
        let mut messages = session.truncated_messages(window);

        // 注入项目记忆
        if let Some(mem) = ctx.project_memory.lock().unwrap().clone() {
            if let Some(first) = messages.first_mut() {
                if matches!(first.role, crate::llm::message::Role::System) {
                    first.content = crate::memory::project::inject_into_system(&first.content, &mem);
                }
            }
        }
        let alias = session.provider_alias.clone();
        let provider = models.get(&alias).cloned();
        let max = provider
            .as_ref()
            .and_then(|p| p.max_tokens)
            .unwrap_or(models.settings.max_tokens_limit);
        let temp = provider.as_ref().and_then(|p| p.temperature);
        (messages, alias, Some(max), temp)
    };

    let req = crate::llm::provider::CompletionRequest {
        messages,
        tools: vec![],
        temperature,
        max_tokens,
        force_non_stream: true,
    };

    let (used_alias, resp) = ctx.chain.read().unwrap().chat_with_fallback(&model_alias, req).await?;
    if !used_alias.is_empty() && used_alias != model_alias {
        crate::ui::colors::print_info(&format!("⚠️  已自动降级到 provider `{used_alias}`"));
    }
    print!("{}", resp.content);
    if !resp.content.ends_with('\n') {
        println!();
    }

    ctx.session
        .lock()
        .unwrap()
        .push_assistant(resp.content.trim());
    let _ = ctx.session.lock().unwrap().save();

    Ok(0)
}
