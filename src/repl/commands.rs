//! 内置命令实现。

use crate::agent::ThinkingMode;
use crate::config::settings::Settings;
use crate::memory::compressor::maybe_compact;
use crate::memory::evolution;
use crate::memory::project as project_memory;
use crate::repl::command::CmdOutcome;
use crate::repl::context::AppContext;
use crate::session::chat::ChatSession;
use crate::tools::web_search::default_provider;
use crate::ui::colors;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

pub const HELP_TEXT: &str = r#"
fr-cli 内置命令
================

会话管理
  /new [名称]             创建新会话
  /save [名称]            保存当前会话到 ~/.fr_cli/sessions/<名称>.json
  /load <名称>            加载历史会话
  /list_sessions          列出所有已保存会话（按更新时间倒序）
  /see [n]                预览最近 n 条消息（默认 5）

模型与配置
  /model [别名]           列出或切换 provider
  /providers              列出所有 provider
  /key <别名> <key>       设置 API key
  /lang <zh|en>           切换 system prompt 语言
  /limit <n>              单轮 max_tokens
  /autonomous <on|off>    切换自治模式
  /mode <direct|cot|tot|react|plan>  切换思维模式（影响 system prompt 与 agent 行为）
  /memory [show|reload]   显示或重新加载项目记忆（.frcli.md / AGENTS.md / CLAUDE.md）
  /compact                立即压缩老对话到摘要段

工作目录与文件
  /dir [路径]             切换工作目录
  /read <文件>            读文件到终端
  /write <文件> <内容>    写新内容到文件（覆盖）
  /shell <cmd>            执行 shell（autonomous on 时免确认）

自我记忆进化 ⭐ (Round 3 新增)
  /memory_topics show                        列出关注话题
  /memory_topics add <name>                  增加话题
  /memory_topics rm <name>                   删除话题
  /memory_evolve                             立刻跑一次搜索进化
  /memory_evolve auto on [secs]              启动后台定时进化（默认 1800s=30min）
  /memory_evolve auto off                    停止

其他
  /banner /version /clear /doctor /help /exit

直接输入文字 = 与 AI 对话。LLM 可见工具：
  read_file / write_file / list_dir / shell / web_search / memorize / recall /
  enter_plan_mode / exit_plan_mode / spawn_agent / task_output

授权交互：工具请求时输入 y=这一次 / a=always / f=full-auto / n=no。
"#;

pub async fn mode(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    if args.is_empty() {
        let cur = *ctx.thinking.lock().unwrap();
        println!("  当前 /mode = {}", cur.label());
        println!("  可选: direct | cot | tot | react | plan");
        return Ok(CmdOutcome::Continue);
    }
    let m = match ThinkingMode::parse(args[0]) {
        Some(m) => m,
        None => {
            colors::print_error("用法: /mode direct|cot|tot|react|plan");
            return Ok(CmdOutcome::Continue);
        }
    };
    *ctx.thinking.lock().unwrap() = m;
    colors::print_info(&format!(
        "✓ /mode 已切到 `{}`",
        m.label()
    ));
    Ok(CmdOutcome::Continue)
}

pub async fn project_memory_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("show");
    match sub {
        "show" => {
            let pm = ctx.project_memory.lock().unwrap().clone();
            match pm {
                Some(m) => {
                    println!();
                    println!("项目记忆（自动加载自 {}）:", m.source_path.display());
                    println!();
                    for line in m.content.lines().take(80) {
                        println!("  {line}");
                    }
                    if m.content.lines().count() > 80 {
                        println!("  …（省略）");
                    }
                    println!();
                }
                None => colors::print_info(
                    "未发现项目记忆。从 cwd 向上找 .frcli.md / AGENTS.md / CLAUDE.md / .github/AGENTS.md。",
                ),
            }
        }
        "reload" => {
            let cwd = ctx.cwd.lock().unwrap().clone();
            match project_memory::discover(&cwd) {
                Ok(Some(m)) => {
                    *ctx.project_memory.lock().unwrap() = Some(m.clone());
                    colors::print_info(&format!(
                        "✓ 重新加载项目记忆自 `{}` ({} chars)",
                        m.source_path.display(),
                        m.content.len()
                    ));
                }
                Ok(None) => colors::print_info("(未发现项目记忆)"),
                Err(e) => colors::print_error(&format!("加载失败: {e}")),
            }
        }
        _ => colors::print_error("用法: /memory [show|reload]"),
    }
    Ok(CmdOutcome::Continue)
}

pub async fn compact(ctx: &AppContext) -> Result<CmdOutcome> {
    let mut s = ctx.session.lock().unwrap();
    let before = s.messages.len();
    maybe_compact(&mut s, 16, 12);
    let after = s.messages.len();
    colors::print_info(&format!(
        "✓ 上下文已压缩: {} → {} 条 (节省 {} 条)",
        before,
        after,
        before.saturating_sub(after)
    ));
    Ok(CmdOutcome::Continue)
}

pub async fn help(_ctx: &AppContext) -> Result<CmdOutcome> {
    print!("{HELP_TEXT}");
    Ok(CmdOutcome::Continue)
}

pub async fn exit(_ctx: &AppContext) -> Result<CmdOutcome> {
    crate::ui::banner::print_bye();
    Ok(CmdOutcome::Exit)
}

pub async fn banner(_ctx: &AppContext) -> Result<CmdOutcome> {
    crate::ui::banner::print_banner();
    Ok(CmdOutcome::Continue)
}

pub async fn version(_ctx: &AppContext) -> Result<CmdOutcome> {
    println!("FrClaw v{}", env!("CARGO_PKG_VERSION"));
    println!("rustc {} on {}", rustc_version_runtime(), std::env::consts::OS);
    Ok(CmdOutcome::Continue)
}

fn rustc_version_runtime() -> &'static str {
    // 静态获取 rustc 版本太重；这里直接留 placeholder。
    "stable"
}

pub async fn clear() -> Result<CmdOutcome> {
    // ANSI 清屏序列；不依赖 tty 类型
    print!("\x1b[2J\x1b[H");
    Ok(CmdOutcome::Continue)
}

// ----------------- 会话 -----------------

pub async fn new_session(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let alias = ctx.chain.primary_alias().unwrap_or("default");
    let name = args.first().copied().unwrap_or("default").to_string();
    let models = ctx.models.lock().unwrap().clone();
    let settings = ctx.settings.lock().unwrap().clone();
    let new = ChatSession::new(name.clone(), &models, &settings, alias);
    *ctx.session.lock().unwrap() = new;
    colors::print_info(&format!("已新建会话 `{name}`"));
    Ok(CmdOutcome::Continue)
}

pub async fn save(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let s = ctx.session.lock().unwrap();
    let name = args
        .first()
        .map(|x| x.to_string())
        .unwrap_or_else(|| s.name.clone());
    drop(s);
    let s = ctx.session.lock().unwrap();
    s.save()?;
    colors::print_info(&format!("会话已保存为 `{name}` ({} messages)", s.messages.len()));
    Ok(CmdOutcome::Continue)
}

pub async fn load(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let name = match args.first() {
        Some(s) => *s,
        None => {
            colors::print_error("用法: /load <name>");
            return Ok(CmdOutcome::Continue);
        }
    };
    let models = ctx.models.lock().unwrap().clone();
    let settings = ctx.settings.lock().unwrap().clone();
    let loaded = ChatSession::load(name, &models, &settings)?;
    *ctx.session.lock().unwrap() = loaded;
    colors::print_info(&format!("会话 `{name}` 已加载"));
    Ok(CmdOutcome::Continue)
}

pub async fn list_sessions(_ctx: &AppContext) -> Result<CmdOutcome> {
    let list = ChatSession::list_all()?;
    println!();
    println!("已保存会话：");
    print!("{}", crate::session::chat::format_session_list(&list));
    println!();
    Ok(CmdOutcome::Continue)
}

pub async fn see(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let n: usize = args
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let session = ctx.session.lock().unwrap();
    println!();
    println!("最近 {n} 条对话：");
    let start = session.messages.len().saturating_sub(n + 1); // include system
    for (i, m) in session.messages.iter().enumerate().skip(start) {
        let role = match m.role {
            crate::llm::message::Role::System => "system",
            crate::llm::message::Role::User => "user",
            crate::llm::message::Role::Assistant => "assistant",
            crate::llm::message::Role::Tool => "tool",
        };
        let preview = crate::session::chat::safe_preview(&m.content, 120);
        println!("  [{i:02}] {role:<9}  {preview}");
    }
    println!();
    Ok(CmdOutcome::Continue)
}

// ----------------- 配置 -----------------

pub async fn model(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let models = ctx.models.lock().unwrap();
    if args.is_empty() {
        println!();
        println!("可用 providers（* = 当前 default, ! = backup）：");
        let def = models.default_provider_name();
        let bk = models.backup_provider_name();
        let cur = ctx.session.lock().unwrap().provider_alias.clone();
        for (alias, p) in models.providers.iter() {
            let mark = match (
                Some(alias) == def.as_ref(),
                Some(alias) == bk.as_ref(),
                alias == &cur,
            ) {
                (_, _, true) => "→",
                (true, _, _) => "*",
                (false, true, _) => "!",
                _ => " ",
            };
            let key = if p.api_key_env.is_some() { "✓" } else { "✗" };
            println!(
                "  {mark} {alias:<14} {name:<20} {model:<28} key:{key}",
                name = p.name,
                model = p.model
            );
        }
        println!();
        println!("  用法: /model <alias>");
        println!();
        return Ok(CmdOutcome::Continue);
    }

    let alias = args[0].to_string();
    if !models.providers.contains_key(&alias) {
        colors::print_error(&format!("未知 provider: {alias}"));
        return Ok(CmdOutcome::Continue);
    }

    drop(models);
    ctx.session.lock().unwrap().provider_alias = alias.clone();
    colors::print_info(&format!("已切换到 `{alias}`"));
    Ok(CmdOutcome::Continue)
}

pub async fn providers(ctx: &AppContext) -> Result<CmdOutcome> {
    model(ctx, &[]).await
}

pub async fn key(_ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    // 用法: /key <alias> <key>
    if args.len() < 2 {
        colors::print_error("用法: /key <alias> <key>");
        return Ok(CmdOutcome::Continue);
    }
    let alias = args[0];
    let key = args[1..].join(" ");
    crate::config::keys::set_key(alias, &key)?;
    colors::print_info(&format!("已为 `{alias}` 保存 key (写入 ~/.fr_cli/keys.json)"));
    Ok(CmdOutcome::Continue)
}

pub async fn lang(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    if args.is_empty() {
        let settings = ctx.settings.lock().unwrap();
        println!("  当前 lang = {}", settings.lang);
        return Ok(CmdOutcome::Continue);
    }
    let new = args[0].to_string();
    let mut settings = ctx.settings.lock().unwrap();
    settings.lang = new.clone();
    drop(settings);
    crate::config::settings::save(&ctx.settings.lock().unwrap())?;

    // 重建 system prompt
    let sys_prompt = crate::llm::prompts::default_system_prompt(&new);
    let mut session = ctx.session.lock().unwrap();
    if let Some(first) = session.messages.first_mut() {
        first.content = sys_prompt.clone();
    } else {
        session.messages.insert(0, crate::llm::message::Message::system(sys_prompt));
    }
    colors::print_info(&format!("已切换语言为 `{new}`, system prompt 已重建"));
    Ok(CmdOutcome::Continue)
}

pub async fn limit(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    if args.is_empty() {
        let settings = ctx.settings.lock().unwrap();
        println!("  当前 limit = {}", settings.limit);
        return Ok(CmdOutcome::Continue);
    }
    let n: u32 = args[0].parse().unwrap_or(8192);
    let mut settings = ctx.settings.lock().unwrap();
    settings.limit = n;
    drop(settings);
    let s = ctx.settings.lock().unwrap().clone();
    crate::config::settings::save(&s)?;
    colors::print_info(&format!("已设置 limit = {n}"));
    Ok(CmdOutcome::Continue)
}

pub async fn autonomous(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let mut settings = ctx.settings.lock().unwrap();
    if args.is_empty() {
        println!("  当前 autonomous = {}", settings.autonomous);
        return Ok(CmdOutcome::Continue);
    }
    let on = match args[0] {
        "on" | "true" | "1" => true,
        "off" | "false" | "0" => false,
        _ => {
            colors::print_error("用法: /autonomous on|off");
            return Ok(CmdOutcome::Continue);
        }
    };
    settings.autonomous = on;
    drop(settings);
    let s = ctx.settings.lock().unwrap().clone();
    crate::config::settings::save(&s)?;
    colors::print_info(&format!("autonomous = {on}"));
    Ok(CmdOutcome::Continue)
}

// ----------------- 文件 / shell -----------------

pub async fn shell(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    if args.is_empty() {
        colors::print_error("用法: /shell <cmd>");
        return Ok(CmdOutcome::Continue);
    }
    let cmd = args.join(" ");
    let cwd = ctx.cwd.lock().unwrap().clone();

    if !ctx.settings.lock().unwrap().autonomous {
        colors::print_info(&format!("即将执行:`{cmd}` (cwd: {})", cwd.display()));
        print!("确认执行？(y/N): ");
        use std::io::Write as _;
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok();
        if line.trim().to_ascii_lowercase() != "y" {
            colors::print_info("已取消");
            return Ok(CmdOutcome::Continue);
        }
    }

    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .current_dir(&cwd)
        .output();

    match output {
        Ok(out) => {
            print!("{}", String::from_utf8_lossy(&out.stdout));
            if !out.stderr.is_empty() {
                eprint!("{}", String::from_utf8_lossy(&out.stderr));
            }
            colors::print_info(&format!(
                "(exit {})",
                out.status.code().unwrap_or(-1)
            ));
        }
        Err(e) => colors::print_error(&format!("执行失败: {e}")),
    }
    Ok(CmdOutcome::Continue)
}

pub async fn read(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let path = match args.first() {
        Some(s) => *s,
        None => {
            colors::print_error("用法: /read <文件>");
            return Ok(CmdOutcome::Continue);
        }
    };
    let cwd = ctx.cwd.lock().unwrap().clone();
    let full = resolve_path(path, &cwd);
    match std::fs::read_to_string(&full) {
        Ok(s) => {
            println!("──── {} ────", full.display());
            if s.len() > 50_000 {
                println!("{} …(截断)", &s[..50_000]);
            } else {
                print!("{s}");
            }
            println!();
        }
        Err(e) => colors::print_error(&format!("读取失败: {e}")),
    }
    Ok(CmdOutcome::Continue)
}

pub async fn write(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    if args.len() < 2 {
        colors::print_error("用法: /write <文件> <内容>");
        return Ok(CmdOutcome::Continue);
    }
    let file = args[0];
    let content = args[1..].join(" ");
    let cwd = ctx.cwd.lock().unwrap().clone();
    let full = resolve_path(file, &cwd);

    // 安全检查：在自治模式关闭时弹授权。
    if !ctx.settings.lock().unwrap().autonomous {
        colors::print_info(&format!("即将覆盖写文件:`{}`", full.display()));
        print!("确认？(y/N): ");
        use std::io::Write as _;
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok();
        if line.trim().to_ascii_lowercase() != "y" {
            colors::print_info("已取消");
            return Ok(CmdOutcome::Continue);
        }
    }

    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    match std::fs::write(&full, content.as_bytes()) {
        Ok(_) => colors::print_info(&format!("写入成功: {}", full.display())),
        Err(e) => colors::print_error(&format!("写入失败: {e}")),
    }
    Ok(CmdOutcome::Continue)
}

pub async fn dir(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    if args.is_empty() {
        let cwd = ctx.cwd.lock().unwrap();
        println!("{}", cwd.display());
        return Ok(CmdOutcome::Continue);
    }
    let path = PathBuf::from(args[0]);
    if !path.exists() {
        colors::print_error(&format!("路径不存在: {}", path.display()));
        return Ok(CmdOutcome::Continue);
    }
    *ctx.cwd.lock().unwrap() = path.clone();
    colors::print_info(&format!("cwd = {}", path.display()));
    Ok(CmdOutcome::Continue)
}

// ----------------- usage / doctor -----------------

pub async fn usage(_ctx: &AppContext) -> Result<CmdOutcome> {
    // 当前 MVP 不持久化 usage 数据。
    colors::print_info("(本期 MVP 不持久化 usage；正式版将累计 prompt/completion tokens)");
    Ok(CmdOutcome::Continue)
}

pub async fn doctor(ctx: &AppContext) -> Result<CmdOutcome> {
    println!();
    println!("fr-cli doctor");
    println!("─────────────");

    let models = ctx.models.lock().unwrap();
    println!(
        "  providers: {} 个 (default={:?}, backup={:?})",
        models.providers.len(),
        models.default_provider_name(),
        models.backup_provider_name()
    );
    for (alias, p) in models.providers.iter() {
        let env_name = p.api_key_env.as_deref().unwrap_or("-");
        let env_present = p
            .api_key_env
            .as_deref()
            .and_then(|e| std::env::var(e).ok())
            .is_some();
        println!(
            "  [{alias}] {name} / {model} protocol={protocol} env={env_name}={has}",
            name = p.name,
            model = p.model,
            protocol = p.protocol,
            has = if env_present { "✓" } else { "✗" }
        );
    }

    let session = ctx.session.lock().unwrap();
    println!(
        "  当前会话: {} ({} messages)",
        session.name,
        session.messages.len()
    );

    let cwd = ctx.cwd.lock().unwrap();
    println!("  cwd: {}", cwd.display());

    println!("─────────────");
    Ok(CmdOutcome::Continue)
}

fn resolve_path(p: &str, cwd: &std::path::Path) -> PathBuf {
    if p.starts_with('/') || p.starts_with('~') {
        PathBuf::from(p)
    } else {
        cwd.join(p)
    }
}

// ----------------- 自我记忆进化 (Round 3) -----------------

pub async fn memory_topics(_ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("show");
    match sub {
        "show" | "ls" | "list" | "" => {
            let t = evolution::TopicsFile::load_or_default();
            println!();
            if t.topics.is_empty() {
                colors::print_info("还没有关注话题。试试 `/memory_topics add rust`");
            } else {
                println!("关注话题（{} 个）：", t.topics.len());
                for item in t.topics.iter() {
                    let last = item
                        .last_evolved_at
                        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "(never)".to_string());
                    let src = &item.source;
                    println!(
                        "  • {}   last_evolved: {}   [{}]",
                        item.name, last, src
                    );
                }
            }
            println!();
        }
        "add" => {
            let name = match args.get(1) {
                Some(s) => *s,
                None => {
                    colors::print_error("用法: /memory_topics add <name>");
                    return Ok(CmdOutcome::Continue);
                }
            };
            match evolution::add_topic(name) {
                Ok(t) => colors::print_info(&format!(
                    "✓ 已添加关注话题 `{}`",
                    t.name
                )),
                Err(e) => colors::print_error(&format!("添加失败: {e}")),
            }
        }
        "rm" | "remove" | "del" => {
            let name = match args.get(1) {
                Some(s) => *s,
                None => {
                    colors::print_error("用法: /memory_topics rm <name>");
                    return Ok(CmdOutcome::Continue);
                }
            };
            match evolution::remove_topic(name) {
                Ok(true) => colors::print_info(&format!("✓ 已删除 `{name}`")),
                Ok(false) => colors::print_info(&format!("话题 `{name}` 不存在")),
                Err(e) => colors::print_error(&format!("删除失败: {e}")),
            }
        }
        _ => colors::print_error("用法: /memory_topics [show|add|rm]"),
    }
    Ok(CmdOutcome::Continue)
}

pub async fn memory_evolve(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("once");

    match sub {
        "once" | "now" | "" => {
            run_evolution_once(ctx, 3).await;
        }
        "auto" => {
            // /memory_evolve auto on [secs]
            let on_off = args.get(1).copied().unwrap_or("");
            match on_off {
                "on" | "start" => {
                    let secs: u64 = args
                        .get(2)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(1800);
                    let mut guard = ctx.evolution_auto.lock().unwrap();
                    if let Some(prev) = guard.as_ref() {
                        if prev.is_running() {
                            colors::print_info("auto 已在运行；先 stop 再 start");
                            return Ok(CmdOutcome::Continue);
                        }
                    }
                    let provider: Arc<dyn crate::tools::web_search::WebSearchProvider> =
                        Arc::from(default_provider());
                    let auto = evolution::start_auto(provider, 3, secs);
                    *guard = Some(auto);
                    drop(guard);
                    colors::print_info(&format!(
                        "✓ 进化 auto timer 已启动：每 {secs}s 跑一次 evolution"
                    ));
                }
                "off" | "stop" => {
                    let mut guard = ctx.evolution_auto.lock().unwrap();
                    if let Some(auto) = guard.take() {
                        auto.stop();
                        colors::print_info("✓ 进化 auto timer 已停");
                    } else {
                        colors::print_info("auto 没在跑");
                    }
                }
                _ => {
                    let running = ctx
                        .evolution_auto
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|a| a.is_running())
                        .unwrap_or(false);
                    println!(
                        "  evolution auto 状态: {}",
                        if running { "running" } else { "stopped" }
                    );
                    println!("  用法: /memory_evolve auto on [secs]  (默认 1800s)");
                }
            }
        }
        _ => colors::print_error("用法: /memory_evolve [once|auto on|auto off]"),
    }
    Ok(CmdOutcome::Continue)
}

async fn run_evolution_once(ctx: &AppContext, per_topic_limit: usize) {
    let topics = evolution::TopicsFile::load_or_default().topics;
    if topics.is_empty() {
        colors::print_info("还没有话题，先 `/memory_topics add <name>`");
        return;
    }

    let provider: Arc<dyn crate::tools::web_search::WebSearchProvider> =
        Arc::from(default_provider());
    let backend_name = provider.name();
    colors::print_info(&format!(
        "⏳ 进化中：{} 个话题，backend = {}",
        topics.len(),
        backend_name
    ));
    println!();

    match evolution::run_evolution(provider.as_ref(), per_topic_limit).await {
        Ok(r) => {
            println!();
            for p in &r.per_topic {
                let hits_n = p.hits.len();
                if hits_n == 0 {
                    println!("  · {}  (0 hits)", p.topic);
                    continue;
                }
                println!("  · {}  ({} hits)", p.topic, hits_n);
                for (i, hit) in p.hits.iter().take(3).enumerate() {
                    println!("      {}. {}  {}", i + 1, hit.title, hit.url);
                }
            }
            println!();
            colors::print_info(&format!(
                "✓ 进化完成  快照写入 {}  顺带 append 到 long-term.md",
                r.snapshot_path.display()
            ));
            println!();

            // 把刚 append 的内容预热进 ctx.project_memory（强制下次注入新片段）
            // 简化：直接刷新 long_term snippet 到下一次 session.messages[0]
            let long_term = evolution::load_long_term_snippet();
            if !long_term.is_empty() {
                let mut s = ctx.session.lock().unwrap();
                if let Some(first) = s.messages.first_mut() {
                    if matches!(first.role, crate::llm::message::Role::System) {
                        // 在 system 末尾追加「长期记忆」段；项目记忆跟在后面
                        let marker = "\n# 长期记忆（来自 self-evolve）";
                        if !first.content.contains(marker) {
                            first.content.push_str(marker);
                            first.content.push('\n');
                            first.content.push_str(&long_term);
                        }
                    }
                }
            }
        }
        Err(e) => colors::print_error(&format!("进化失败: {e:#}")),
    }
}

// 静默未用到警告
#[allow(dead_code)]
fn _settings_ref(s: &Settings) -> &Settings {
    s
}

// ----------------- Hooks (Round 4) -----------------

pub async fn hooks_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("show");
    match sub {
        "show" | "ls" | "" => {
            // 展示当前 hooks.json 内容 + 触发事件概览
            let cfg = ctx.hooks.lock().unwrap().clone();
            println!();
            println!("Hooks 配置（来自 {}）:", crate::hooks::config::hooks_json_path().display());
            println!();
            let mut any = false;
            for ev_label in [
                crate::hooks::events::HookEvent::PreToolUse,
                crate::hooks::events::HookEvent::PostToolUse,
                crate::hooks::events::HookEvent::UserPromptSubmit,
                crate::hooks::events::HookEvent::SessionStart,
            ] {
                let entries = cfg.entries_for(ev_label);
                if entries.is_empty() {
                    continue;
                }
                any = true;
                println!("  [{}]  ({} 条)", ev_label.label(), entries.len());
                for (i, e) in entries.iter().enumerate() {
                    println!("    {}. matcher=`{}` ({} hooks)", i + 1, e.matcher, e.hooks.len());
                    for h in &e.hooks {
                        let cmd_preview = if h.command.chars().count() > 80 {
                            let mut s: String = h.command.chars().take(80).collect();
                            s.push('…');
                            s
                        } else {
                            h.command.clone()
                        };
                        println!("       - shell: {}  (timeout {}ms)", cmd_preview, h.timeout_ms);
                    }
                }
                println!();
            }
            if !any {
                println!("  (没有注册任何 hook — 用 `fr-cli hooks add` 加示例)");
            }
        }
        "events" => {
            println!();
            println!("Hook 事件类型（每条 hook 必须挂在下面某个事件下）：");
            println!();
            println!("  PreToolUse        工具调用 **之前** 触发");
            println!("                    · stdout JSON envelope.modified_args → 改 tool_args");
            println!("                    · exit 2 / continue_=false → **阻止**该调用");
            println!("  PostToolUse       工具调用 **之后** 触发（stdout 一般被忽略，仅做 audit）");
            println!("  UserPromptSubmit  用户文本送进 LLM 前触发");
            println!("                    · additional_context → 拼到 user 消息尾部");
            println!("                    · modified_prompt → 替换原 prompt");
            println!("                    · exit 2 → **阻止**该消息进入");
            println!("  SessionStart      REPL 启动时跑一次（stdout 自由）");
            println!();
            println!("hook 命令通过 stdin 收 HookInput JSON。env 提供：");
            println!("    FR_HOOK_EVENT       PreToolUse / PostToolUse / ...");
            println!("    FR_HOOK_TIMESTAMP_MS 当前时间戳 (Unix ms)");
            println!();
        }
        "init" | "sample" => {
            match crate::hooks::config::ensure_sample() {
                Ok(true) => {
                    let new_cfg = crate::hooks::HooksFile::load_or_default();
                    *ctx.hooks.lock().unwrap() = new_cfg;
                    colors::print_info(&format!(
                        "✓ 已生成样板 hooks.json: {}",
                        crate::hooks::config::hooks_json_path().display()
                    ))
                }
                Ok(false) => colors::print_info("hooks.json 已存在，未覆盖"),
                Err(e) => colors::print_error(&format!("写样板失败: {e}")),
            }
        }
        "path" => {
            println!("  {}", crate::hooks::config::hooks_json_path().display());
        }
        "reload" => {
            let new_cfg = crate::hooks::HooksFile::load_or_default();
            let count = new_cfg.pre_tool_use.len()
                + new_cfg.post_tool_use.len()
                + new_cfg.user_prompt_submit.len()
                + new_cfg.session_start.len();
            *ctx.hooks.lock().unwrap() = new_cfg;
            colors::print_info(&format!(
                "✓ 已 hot-reload hooks.json ({count} 个 entry)"
            ));
        }
        _ => colors::print_error("用法: /hooks [show|events|init|reload|path]"),
    }
    Ok(CmdOutcome::Continue)
}// ----------------- Skills (Round 5) -----------------

pub async fn skill_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" | "ls" | "" => {
            let all = ctx.skills.all();
            println!();
            println!("已发现 skill（{} 个）：", all.len());
            println!();
            for s in &all {
                let kind = match s.kind {
                    crate::skills::loader::SkillSource::Builtin => "builtin",
                    crate::skills::loader::SkillSource::User => "user  ",
                };
                let descr = if s.frontmatter.description.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", s.frontmatter.description)
                };
                let triggers = if s.frontmatter.triggers.is_empty() {
                    String::new()
                } else {
                    format!("  [{}]", s.frontmatter.triggers.join(" | "))
                };
                println!(
                    "  • {:<22}  ({}){} {}",
                    s.frontmatter.name, kind, descr, triggers
                );
            }
            println!();
            println!(
                "  用户目录: {}",
                crate::skills::registry::user_skills_dir().display()
            );
            if let Ok(bp) = std::fs::read_dir(crate::skills::registry::builtin_skills_dir()) {
                if bp.count() > 0 {
                    println!(
                        "  builtin 目录: {}",
                        crate::skills::registry::builtin_skills_dir().display()
                    );
                }
            }
            println!();
        }
        "show" => {
            let name = match args.get(1) {
                Some(s) => *s,
                None => {
                    colors::print_error("用法: /skill show <name>");
                    return Ok(CmdOutcome::Continue);
                }
            };
            match ctx.skills.get(name) {
                Some(s) => {
                    println!();
                    println!("─── Skill: {} ───", s.frontmatter.name);
                    println!("路径:   {}", s.source_path);
                    println!("来源:   {:?}", s.kind);
                    println!(
                        "trigger: {}",
                        s.frontmatter.triggers.join(" | ")
                    );
                    if !s.frontmatter.allowed_tools.is_empty() {
                        println!(
                            "tools:  {}",
                            s.frontmatter.allowed_tools.join(", ")
                        );
                    }
                    println!("max-steps: {}", s.frontmatter.max_steps);
                    if !s.frontmatter.description.is_empty() {
                        println!("\n> {}", s.frontmatter.description);
                    }
                    println!();
                    println!("{}", s.body);
                    println!();
                }
                None => colors::print_error(&format!("未知 skill: {name}")),
            }
        }
        "dir" => {
            let dir = crate::skills::registry::user_skills_dir();
            println!("  {}", dir.display());
            if !dir.exists() {
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    colors::print_error(&format!("建目录失败: {e}"));
                } else {
                    colors::print_info(&format!("✓ 已创建: {}", dir.display()));
                }
            }
        }
        "reload" => match ctx.skills.reload() {
            Ok(n) => colors::print_info(&format!("✓ hot-reload 完成：{n} 条 skill")),
            Err(e) => colors::print_error(&format!("reload 失败: {e}")),
        },
        "path" => {
            println!("  user:   {}", crate::skills::registry::user_skills_dir().display());
            println!(
                "  builtin: {}",
                crate::skills::registry::builtin_skills_dir().display()
            );
        }
        _ => colors::print_error("用法: /skill [list|show <name>|dir|reload|path]"),
    }
    Ok(CmdOutcome::Continue)
}// ----------------- MCP (Round 6) -----------------

pub async fn mcp_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" | "ls" | "" => {
            let servers = ctx.mcp.list_servers().await;
            println!();
            println!("MCP servers（{} 个）：", servers.len());
            println!();
            if servers.is_empty() {
                println!("  (还没有配置 — `/mcp add <name> <url>` 添)");
            } else {
                for s in &servers {
                    let status = match s.status {
                        crate::mcp::registry::ConnectionStatus::Pending => "pending",
                        crate::mcp::registry::ConnectionStatus::Connected => "connected",
                        crate::mcp::registry::ConnectionStatus::Failed => "failed",
                        crate::mcp::registry::ConnectionStatus::Disabled => "disabled",
                    };
                    println!("  • {:<20}  {:<10}  {}", s.name, status, s.url);
                    if let Some(err) = &s.last_error {
                        println!("      error: {err}");
                    }
                    if !s.tools.is_empty() {
                        println!("      tools ({}):", s.tools.len());
                        for t in s.tools.iter().take(5) {
                            let descr = t.description.as_deref().unwrap_or("").chars().take(60).collect::<String>();
                            println!(
                                "        - {}  {}",
                                t.qualified_name(&s.name),
                                descr
                            );
                        }
                        if s.tools.len() > 5 {
                            println!("        ... ({} more)", s.tools.len() - 5);
                        }
                    }
                }
            }
            println!();
        }
        "add" => {
            let name = match args.get(1) {
                Some(s) => *s,
                None => {
                    colors::print_error("用法: /mcp add <name> <url>");
                    return Ok(CmdOutcome::Continue);
                }
            };
            let url = match args.get(2) {
                Some(s) => *s,
                None => {
                    colors::print_error("用法: /mcp add <name> <url>");
                    return Ok(CmdOutcome::Continue);
                }
            };
            let cfg = crate::mcp::config::McpServersFile {
                servers: vec![crate::mcp::config::McpServerConfig {
                    name: name.into(),
                    url: url.into(),
                    auto_connect: true,
                    enabled: true,
                    env: Default::default(),
                    headers: Default::default(),
                    timeout_ms: None,
                }],
            };
            // 合并：append 到现有文件
            let mut existing = crate::mcp::config::McpServersFile::load_or_default();
            if existing.servers.iter().any(|s| s.name == name) {
                colors::print_error(&format!("已存在 MCP server `{name}` — 先 remove"));
                return Ok(CmdOutcome::Continue);
            }
            existing.servers.extend(cfg.servers);
            match existing.save() {
                Ok(_) => colors::print_info(&format!(
                    "✓ 已添加 `{name}` -> {url}\n  用 `/mcp reconnect {name}` 立即连上"
                )),
                Err(e) => colors::print_error(&format!("保存失败: {e}")),
            }
        }
        "remove" | "rm" => {
            let name = match args.get(1) {
                Some(s) => *s,
                None => {
                    colors::print_error("用法: /mcp remove <name>");
                    return Ok(CmdOutcome::Continue);
                }
            };
            let mut existing = crate::mcp::config::McpServersFile::load_or_default();
            let n_before = existing.servers.len();
            existing.servers.retain(|s| s.name != name);
            if existing.servers.len() == n_before {
                colors::print_error(&format!("未知 MCP server `{name}`"));
                return Ok(CmdOutcome::Continue);
            }
            match existing.save() {
                Ok(_) => colors::print_info(&format!("✓ 已删除 `{name}`（重启 REPL 生效）")),
                Err(e) => colors::print_error(&format!("保存失败: {e}")),
            }
        }
        "reconnect" | "rc" => {
            let name = match args.get(1) {
                Some(s) => *s,
                None => {
                    colors::print_error("用法: /mcp reconnect <name>");
                    return Ok(CmdOutcome::Continue);
                }
            };
            colors::print_info(&format!("⏳ reconnect `{name}`..."));
            match ctx.mcp.reconnect(name).await {
                Ok(()) => colors::print_info(&format!("✓ {name} 已重连")),
                Err(e) => colors::print_error(&format!("reconnect 失败: {e}")),
            }
        }
        "tools" => {
            let servers = ctx.mcp.list_servers().await;
            println!();
            println!("MCP tools（按 server 分组）：");
            println!();
            for s in &servers {
                if s.tools.is_empty() {
                    continue;
                }
                println!("  [{}]  ({})", s.name, s.url);
                for t in &s.tools {
                    let descr = t.description.as_deref().unwrap_or("").chars().take(80).collect::<String>();
                    println!(
                        "    - {:<32}  {}",
                        t.qualified_name(&s.name),
                        descr
                    );
                }
                println!();
            }
        }
        "call" => {
            // /mcp call <server> <tool> [args-json]
            let server = match args.get(1) {
                Some(s) => *s,
                None => {
                    colors::print_error("用法: /mcp call <server> <tool> [args-json]");
                    return Ok(CmdOutcome::Continue);
                }
            };
            let tool = match args.get(2) {
                Some(s) => *s,
                None => {
                    colors::print_error("用法: /mcp call <server> <tool> [args-json]");
                    return Ok(CmdOutcome::Continue);
                }
            };
            let args_value: serde_json::Value = match args.get(3) {
                Some(s) => match serde_json::from_str(s) {
                    Ok(v) => v,
                    Err(e) => {
                        colors::print_error(&format!("args JSON 解析失败: {e}"));
                        return Ok(CmdOutcome::Continue);
                    }
                },
                None => serde_json::json!({}),
            };
            let qname = crate::mcp::protocol::Tool {
                name: tool.into(),
                description: None,
                input_schema: serde_json::json!({}),
            }
            .qualified_name(server);
            match ctx.mcp.dispatch(&qname, args_value).await {
                Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default()),
                Err(e) => colors::print_error(&format!("调 MCP 失败: {e}")),
            }
        }
        "config" | "path" => {
            println!("  {}", crate::mcp::config::servers_json_path().display());
        }
        "resources" | "rs" => {
            let all = ctx.mcp.all_resources().await;
            if all.is_empty() {
                colors::print_info("(没有连上任何 resource)");
            } else {
                println!();
                println!("MCP resources（{} 个）：", all.len());
                println!();
                let mut by_server: std::collections::BTreeMap<String, Vec<_>> =
                    std::collections::BTreeMap::new();
                for (server, r) in &all {
                    by_server.entry(server.clone()).or_default().push(r);
                }
                for (server, rs) in &by_server {
                    println!("  [{server}]  ({} 个)", rs.len());
                    for r in rs {
                        let name = r.name.clone().unwrap_or_else(|| r.uri.clone());
                        let mime = r.mime_type.clone().unwrap_or_default();
                        let descr = r
                            .description
                            .clone()
                            .unwrap_or_default()
                            .chars()
                            .take(60)
                            .collect::<String>();
                        println!("    • {:<28}  {:<14}  {}", name, mime, descr);
                        println!("        uri: {}", r.uri);
                    }
                }
                println!();
            }
        }
        "read-resource" | "rr" => {
            // /mcp read-resource <server> <uri>
            if args.len() < 3 {
                colors::print_error("用法: /mcp read-resource <server> <uri>");
                return Ok(CmdOutcome::Continue);
            }
            let server = args[1];
            let uri = args[2];
            match ctx.mcp.read_resource(server, uri).await {
                Ok(r) => {
                    println!();
                    for c in &r.contents {
                        println!("--- uri: {} ---", c.uri().unwrap_or("?"));
                        println!("{}", c.to_text());
                    }
                    println!();
                }
                Err(e) => colors::print_error(&format!("read_resource 失败: {e:#}")),
            }
        }
        "prompts" | "ps" => {
            let all = ctx.mcp.all_prompts().await;
            if all.is_empty() {
                colors::print_info("(没有连上任何 prompt 模板)");
            } else {
                println!();
                println!("MCP prompts（{} 个）：", all.len());
                println!();
                let mut by_server: std::collections::BTreeMap<String, Vec<_>> =
                    std::collections::BTreeMap::new();
                for (server, p) in &all {
                    by_server.entry(server.clone()).or_default().push(p);
                }
                for (server, ps) in &by_server {
                    println!("  [{server}]  ({} 个)", ps.len());
                    for p in ps {
                        let descr = p
                            .description
                            .clone()
                            .unwrap_or_default()
                            .chars()
                            .take(60)
                            .collect::<String>();
                        println!("    • {:<24}  {}", p.name, descr);
                        if !p.arguments.is_empty() {
                            let req: Vec<&str> = p
                                .arguments
                                .iter()
                                .filter(|a| a.required.unwrap_or(false))
                                .map(|a| a.name.as_str())
                                .collect();
                            println!(
                                "        args ({}): {}{}",
                                p.arguments.len(),
                                p.arguments
                                    .iter()
                                    .map(|a| a.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", "),
                                if !req.is_empty() {
                                    format!("  [required: {}]", req.join(", "))
                                } else {
                                    String::new()
                                }
                            );
                        }
                    }
                }
                println!();
            }
        }
        "get-prompt" | "gp" => {
            // /mcp get-prompt <server> <name> [key=val key=val ...]
            if args.len() < 3 {
                colors::print_error("用法: /mcp get-prompt <server> <name> [k=v ...]");
                return Ok(CmdOutcome::Continue);
            }
            let server = args[1];
            let name = args[2];
            let mut prompt_args = serde_json::Map::new();
            for kv in &args[3..] {
                if let Some((k, v)) = kv.split_once('=') {
                    prompt_args.insert(k.to_string(), serde_json::Value::String(v.to_string()));
                } else {
                    colors::print_error(&format!("bad arg `{kv}` (需要 key=val 形式)"));
                }
            }
            match ctx
                .mcp
                .get_prompt(server, name, Some(serde_json::Value::Object(prompt_args)))
                .await
            {
                Ok(r) => {
                    println!();
                    if let Some(d) = r.description {
                        println!("--- prompt 描述 ---");
                        println!("{d}");
                        println!();
                    }
                    println!("--- messages ({} 条) ---", r.messages.len());
                    for (i, m) in r.messages.iter().enumerate() {
                        println!("[{}] {}:", i + 1, m.role);
                        println!("{}", m.to_text());
                        println!();
                    }
                }
                Err(e) => colors::print_error(&format!("get_prompt 失败: {e:#}")),
            }
        }
        "refresh" | "rf" => {
            // /mcp refresh <server>
            if args.len() < 2 {
                colors::print_error("用法: /mcp refresh <server>");
                return Ok(CmdOutcome::Continue);
            }
            let server = args[1];
            let _ = ctx.mcp.refresh_resources(server).await;
            let _ = ctx.mcp.refresh_prompts(server).await;
            colors::print_info(&format!("✓ 已刷新 {server} 的 resources + prompts"));
        }
        _ => colors::print_error(
            "用法: /mcp [list|add|remove|reconnect|refresh|tools|call|resources|read-resource|prompts|get-prompt|config]",
        ),
    }
    Ok(CmdOutcome::Continue)
}

// ─── Round 7 ─ RAG 知识库 ──────────────────────────────────────────

pub async fn rag_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" | "ls" | "" => {
            let sources = ctx.rag.list().unwrap_or_default();
            println!();
            println!("RAG sources（{} 个，{} chunks）：", sources.len(), ctx.rag.count().unwrap_or(0));
            println!();
            if sources.is_empty() {
                println!("  (空 — `/rag add <source> <text>` 或 `/rag import <path>` 入库)");
            } else {
                for s in &sources {
                    let ts = chrono::DateTime::from_timestamp(s.last_updated, 0)
                        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "?".to_string());
                    println!("  • {:<30} {:>4} chunks   updated {}", s.name, s.count, ts);
                }
            }
            println!();
            println!("  auto-recall: {}", if ctx.rag.auto() { "on" } else { "off" });
            println!("  切换: /rag auto on | off");
        }
        "add" => {
            // /rag add <source> <text...>
            if args.len() < 3 {
                colors::print_error("用法: /rag add <source> <text...>");
                return Ok(CmdOutcome::Continue);
            }
            let source = args[1];
            let text = args[2..].join(" ");
            match ctx.rag.add(source, &text, None) {
                Ok(n) => colors::print_info(&format!("✓ {source} → {n} chunks")),
                Err(e) => colors::print_error(&format!("rag add 失败: {e:#}")),
            }
        }
        "query" | "q" => {
            // /rag query [--hybrid] [--vec-w 0.5] <text...>
            if args.len() < 2 {
                colors::print_error("用法: /rag query [--hybrid|--vector] [--vec-w 0.5] <text...>");
                return Ok(CmdOutcome::Continue);
            }
            let mut use_hybrid = true;
            let mut vec_w = 0.5f32;
            let mut text_parts: Vec<&str> = Vec::new();
            for a in &args[1..] {
                match *a {
                    "--hybrid" => use_hybrid = true,
                    "--vector" => use_hybrid = false,
                    _ if a.starts_with("--vec-w=") => {
                        if let Some(n) = a.strip_prefix("--vec-w=") {
                            if let Ok(v) = n.parse::<f32>() {
                                vec_w = v.clamp(0.0, 1.0);
                            }
                        }
                    }
                    _ => text_parts.push(a),
                }
            }
            let q = text_parts.join(" ");
            if q.is_empty() {
                colors::print_error("query 文本不能为空");
                return Ok(CmdOutcome::Continue);
            }
            // 先拿 5x 候选
            let n_candidates = 25;
            let raw_hits = match ctx.rag.query(&q, n_candidates) {
                Ok(v) => v,
                Err(e) => {
                    colors::print_error(&format!("rag query 失败: {e:#}"));
                    return Ok(CmdOutcome::Continue);
                }
            };
            if use_hybrid {
                let tuples = crate::rag::hybrid::hits_to_tuples(&raw_hits);
                let opts = crate::rag::hybrid::HybridOptions {
                    vector_weight: vec_w,
                    k: 5,
                    ..Default::default()
                };
                let hits = crate::rag::hybrid::hybrid_search(&q, &tuples, &opts);
                if hits.is_empty() {
                    println!("  (无命中)");
                } else {
                    println!();
                    println!("  [hybrid: vec_w={:.2}]  {} hits", vec_w, hits.len());
                    for (i, h) in hits.iter().enumerate() {
                        let preview = if h.content.chars().count() > 160 {
                            let s: String = h.content.chars().take(160).collect();
                            format!("{s}…")
                        } else {
                            h.content.clone()
                        };
                        println!(
                            "  #{i}  hybrid={:.3}  vec={:.3}  bm25={:.3}  {}#{}",
                            h.hybrid_score, h.vector_score, h.bm25_score, h.source, h.chunk_index
                        );
                        println!("      {preview}");
                    }
                    println!();
                }
            } else {
                let hits: Vec<_> = raw_hits.into_iter().take(5).collect();
                if hits.is_empty() {
                    println!("  (无命中)");
                } else {
                    println!();
                    println!("  [vector only]  {} hits", hits.len());
                    for (i, h) in hits.iter().enumerate() {
                        let preview = if h.content.chars().count() > 160 {
                            let s: String = h.content.chars().take(160).collect();
                            format!("{s}…")
                        } else {
                            h.content.clone()
                        };
                        println!("  #{i}  score={:.3}  {}#{}", h.score, h.source, h.chunk_index);
                        println!("      {preview}");
                    }
                    println!();
                }
            }
        }
        "show" => {
            if args.len() < 2 {
                colors::print_error("用法: /rag show <source>");
                return Ok(CmdOutcome::Continue);
            }
            let name = args[1];
            match ctx.rag.get_source(name) {
                Ok(chunks) if chunks.is_empty() => colors::print_error(&format!("source `{name}` 不存在")),
                Ok(chunks) => {
                    println!();
                    for (i, c) in &chunks {
                        println!("  ── chunk {i} ──");
                        println!("{}", c);
                    }
                    println!();
                }
                Err(e) => colors::print_error(&format!("rag show 失败: {e:#}")),
            }
        }
        "remove" | "rm" => {
            if args.len() < 2 {
                colors::print_error("用法: /rag remove <source>");
                return Ok(CmdOutcome::Continue);
            }
            let name = args[1];
            match ctx.rag.remove(name) {
                Ok(n) => colors::print_info(&format!("✓ {name}: 删除 {n} chunks")),
                Err(e) => colors::print_error(&format!("rag remove 失败: {e:#}")),
            }
        }
        "import" => {
            if args.len() < 2 {
                colors::print_error("用法: /rag import <path> [<source_name>]");
                return Ok(CmdOutcome::Continue);
            }
            let path = std::path::Path::new(args[1]);
            if !path.exists() {
                colors::print_error(&format!("路径不存在: {}", path.display()));
                return Ok(CmdOutcome::Continue);
            }
            let source = if args.len() >= 3 {
                args[2].to_string()
            } else {
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "imported".to_string())
            };
            let mut total = 0usize;
            if path.is_file() {
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        match ctx.rag.add(&source, &content, None) {
                            Ok(n) => {
                                total += n;
                                colors::print_info(&format!("✓ {} → {} chunks (total {})", path.display(), n, total));
                            }
                            Err(e) => colors::print_error(&format!("{} 失败: {e:#}", path.display())),
                        }
                    }
                    Err(e) => colors::print_error(&format!("读取 {} 失败: {e}", path.display())),
                }
            } else if path.is_dir() {
                for entry in std::fs::read_dir(path).map_err(|e| anyhow::anyhow!("{e}"))? {
                    let entry = entry?;
                    let p = entry.path();
                    if !p.is_file() {
                        continue;
                    }
                    let ext = p.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
                    if !matches!(ext.as_str(), "md" | "txt" | "json" | "markdown" | "rst") {
                        continue;
                    }
                    match std::fs::read_to_string(&p) {
                        Ok(content) => {
                            let fname = p.file_name().unwrap().to_string_lossy().to_string();
                            match ctx.rag.add(&fname, &content, None) {
                                Ok(n) => {
                                    total += n;
                                    colors::print_info(&format!("  ✓ {} → {} chunks", fname, n));
                                }
                                Err(e) => colors::print_error(&format!("  {fname} 失败: {e:#}")),
                            }
                        }
                        Err(e) => {
                            let fname = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            colors::print_error(&format!("  读取 {} 失败: {}", fname, e))
                        }
                    }
                }
                colors::print_info(&format!("目录导入完成：{total} chunks"));
            }
        }
        "auto" => {
            if args.len() < 2 {
                println!("  auto-recall: {}", if ctx.rag.auto() { "on" } else { "off" });
                return Ok(CmdOutcome::Continue);
            }
            let on = matches!(args[1], "on" | "1" | "true" | "yes");
            let off = matches!(args[1], "off" | "0" | "false" | "no");
            if on {
                ctx.rag.set_auto(true);
                colors::print_info("✓ auto-recall 已开启");
            } else if off {
                ctx.rag.set_auto(false);
                colors::print_info("✓ auto-recall 已关闭");
            } else {
                colors::print_error("用法: /rag auto on | off");
            }
        }
        "status" => {
            let count = ctx.rag.count().unwrap_or(0);
            let sources = ctx.rag.list().unwrap_or_default();
            let db_path = ctx.rag.db_path();
            let size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
            println!();
            println!("RAG status:");
            println!("  db:        {}", db_path.display());
            println!("  size:      {:.2} KB", size as f64 / 1024.0);
            println!("  chunks:    {count}");
            println!("  sources:   {}", sources.len());
            println!("  embedder:  hash-baseline (256-dim, 零网络)");
            println!("  auto:      {}", if ctx.rag.auto() { "on" } else { "off" });
            println!();
        }
        "config" | "path" => {
            println!("  {}", ctx.rag.db_path().display());
        }
        "help" | "-h" | "--help" => {
            println!();
            println!("RAG 知识库命令：");
            println!();
            println!("  /rag list                       列出所有 source");
            println!("  /rag add <source> <text...>      手动入库一段文本");
            println!("  /rag query <text...>             检索 top-5");
            println!("  /rag show <source>              显示某 source 全部 chunk");
            println!("  /rag remove <source>            删除某 source 全部 chunk");
            println!("  /rag import <path> [<name>]     导入文件或目录（.md/.txt/.json/.rst）");
            println!("  /rag auto on|off                开关 auto-recall（用户消息自动 query）");
            println!("  /rag status                     总览");
            println!("  /rag config                     打印 db 路径");
            println!();
        }
        _ => {
            colors::print_error("用法: /rag [list|add|query|show|remove|import|auto|status|config|help]");
        }
    }
    Ok(CmdOutcome::Continue)
}

// ─── Round 8 ─ Worktree + multi_edit ──────────────────────────────

pub async fn worktree_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" | "ls" | "" => {
            // 重新探测
            let cwd = ctx.cwd.lock().unwrap().clone();
            ctx.worktree.refresh(&cwd);
            let v = crate::tools::worktree::tool_worktree_list(&ctx.worktree, &serde_json::json!({}))?;
            println!();
            if let Some(err) = v.get("error") {
                println!("  ⚠ {} （cwd: {}）", err, cwd.display());
                println!();
                return Ok(CmdOutcome::Continue);
            }
            let root = v["git_root"].as_str().unwrap_or("?");
            let count = v["count"].as_u64().unwrap_or(0);
            println!("Git worktrees（{} 个，root: {}）：", count, root);
            println!();
            if let Some(arr) = v["worktrees"].as_array() {
                for wt in arr {
                    let p = wt["path"].as_str().unwrap_or("?");
                    let b = wt["branch"].as_str().unwrap_or("?");
                    let marker = if p == root { "★" } else { " " };
                    println!("  {} {:<50} {}", marker, p, b);
                }
            }
            println!();
        }
        "create" | "new" => {
            if args.len() < 2 {
                colors::print_error("用法: /worktree create <path> [<branch>] [<from>]");
                return Ok(CmdOutcome::Continue);
            }
            let path = args[1];
            let branch = args.get(2).copied();
            let from = args.get(3).copied();
            let mut payload = serde_json::json!({ "path": path });
            if let Some(b) = branch {
                payload["branch"] = serde_json::Value::String(b.to_string());
            }
            if let Some(f) = from {
                payload["from"] = serde_json::Value::String(f.to_string());
            }
            let v = crate::tools::worktree::tool_worktree_create(&ctx.worktree, &payload)?;
            if v.get("error").is_some() {
                colors::print_error(&format!("✗ {}", v["error"]));
                if let Some(stderr) = v.get("stderr").and_then(|s| s.as_str()) {
                    eprintln!("    {}", stderr);
                }
            } else {
                colors::print_info(&format!("✓ 创建 worktree: {}", v["path"]));
                if let Some(b) = v.get("branch").and_then(|s| s.as_str()) {
                    if !b.is_empty() {
                        println!("  branch: {b}");
                    }
                }
            }
        }
        "remove" | "rm" => {
            if args.len() < 2 {
                colors::print_error("用法: /worktree remove <path> [--force]");
                return Ok(CmdOutcome::Continue);
            }
            let path = args[1];
            let force = args.iter().any(|a| *a == "--force" || *a == "-f");
            let v = crate::tools::worktree::tool_worktree_remove(
                &ctx.worktree,
                &serde_json::json!({ "path": path, "force": force }),
            )?;
            if v.get("error").is_some() {
                colors::print_error(&format!("✗ {}", v["error"]));
            } else {
                colors::print_info(&format!("✓ 删除 worktree: {}", v["removed"]));
            }
        }
        "status" | "st" => {
            let path = args.get(1).copied();
            let mut payload = serde_json::json!({});
            if let Some(p) = path {
                payload["path"] = serde_json::Value::String(p.to_string());
            }
            let v = crate::tools::worktree::tool_worktree_status(&ctx.worktree, &payload)?;
            if v.get("error").is_some() {
                colors::print_error(&format!("✗ {}", v["error"]));
            } else {
                println!();
                println!("  path:   {}", v["path"]);
                println!("  branch: {}", v["branch"]);
                println!("  clean:  {}", v["clean"]);
                let n = v["dirty_count"].as_u64().unwrap_or(0);
                if n > 0 {
                    println!("  dirty:  {} 文件", n);
                    if let Some(arr) = v["dirty_files"].as_array() {
                        for f in arr {
                            println!("    - {}", f);
                        }
                    }
                }
                println!();
            }
        }
        "root" => {
            if let Some(r) = ctx.worktree.git_root() {
                println!("{}", r.display());
            } else {
                println!("(不在 git 仓库内)");
            }
        }
        "refresh" => {
            let cwd = ctx.cwd.lock().unwrap().clone();
            ctx.worktree.refresh(&cwd);
            match ctx.worktree.git_root() {
                Some(r) => println!("✓ git root: {}", r.display()),
                None => println!("✗ 不在 git 仓库内（cwd: {}）", cwd.display()),
            }
        }
        "help" | "-h" => {
            println!();
            println!("Worktree 命令：");
            println!();
            println!("  /worktree list                  列出所有 worktree（主 + 链接）");
            println!("  /worktree create <path> [<branch>] [<from>]");
            println!("                                  新建 worktree（自动 add + 新分支）");
            println!("  /worktree remove <path> [--force]");
            println!("                                  删除 worktree");
            println!("  /worktree status [path]         看 worktree git 状态");
            println!("  /worktree root                  打印 git root");
            println!("  /worktree refresh               重新探测（cwd 变了后用）");
            println!();
        }
        _ => {
            colors::print_error("用法: /worktree [list|create|remove|status|root|refresh|help]");
        }
    }
    Ok(CmdOutcome::Continue)
}

pub async fn multi_edit_cmd(ctx: &AppContext, rest: &str) -> Result<CmdOutcome> {
    use crate::tools::multi_edit;
    let s = rest.trim();
    if s.is_empty() || s == "help" || s == "-h" {
        println!();
        println!("multi_edit：原子地应用 N 处编辑到 M 个文件。任一失败全部回滚。");
        println!();
        println!("用法：");
        println!();
        println!("  /multi_edit <inline-json>");
        println!("  /multi_edit @/path/to/edits.json");
        println!();
        println!("JSON schema：");
        println!(r#"  {{"edits":[{{"path":"/abs/a.rs","old_text":"...","new_text":"..."}}],"create_if_missing":false}}"#);
        println!();
        println!("  - old_text 在文件里必须**精确出现 1 次**（0 次 / N>1 次都报错）");
        println!("  - 同一文件多次 edit 顺序应用");
        println!("  - 任一失败 → 还原所有已修改的文件");
        println!("  - 成功不留 backup；失败会留 `.bak.fr_multi_edit`");
        println!();
        return Ok(CmdOutcome::Continue);
    }
    let op = match multi_edit::parse_edit_op_from_args(s) {
        Ok(op) => op,
        Err(e) => {
            colors::print_error(&format!("{e:#}"));
            return Ok(CmdOutcome::Continue);
        }
    };
    let result = multi_edit::run_multi_edit(&op);
    if result.ok {
        colors::print_info(&format!(
            "✓ 应用 {} 处编辑，{} 个文件",
            result.applied, result.files_touched
        ));
        for fr in &result.file_results {
            println!(
                "  • {} ({} 处, {}→{} 字节)",
                fr.path, fr.edits_applied, fr.bytes_before, fr.bytes_after
            );
        }
    } else {
        colors::print_error(&format!("✗ {}", result.error.as_deref().unwrap_or("失败")));
        if !result.rolled_back.is_empty() {
            println!("  ↩ 已回滚 {} 个文件:", result.rolled_back.len());
            for p in &result.rolled_back {
                println!("    - {p}");
            }
            println!("  (pre-edit 备份：<file>.bak.fr_multi_edit)");
        }
    }
    let _ = ctx; // 未用
    Ok(CmdOutcome::Continue)
}

// ─── Round 9 ─ Sandbox ────────────────────────────────────────────

pub async fn sandbox_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("status");
    match sub {
        "status" | "" => {
            let p = ctx.sandbox.lock().unwrap().clone();
            println!();
            println!("Sandbox 策略：");
            println!("  enabled:    {}", p.enabled);
            println!("  network:    {:?}", p.network);
            println!("  timeout:    {}ms", p.timeout_ms);
            println!("  max_stdout: {} bytes", p.max_stdout_bytes);
            println!("  macOS exec: {} (supported: {})", p.use_macos_sandbox_exec, crate::sandbox::macos::is_supported());
            println!();
            println!("  read_allow ({} 条):", p.read_allow.len());
            for s in &p.read_allow {
                println!("    - {s}");
            }
            println!("  write_allow ({} 条):", p.write_allow.len());
            for s in &p.write_allow {
                println!("    - {s}");
            }
            println!("  shell_deny ({} 条):", p.shell_deny.len());
            for s in &p.shell_deny {
                println!("    - {s}");
            }
            println!();
            println!("  配置文件: {}", crate::config::paths::sandbox_json_path().display());
            println!();
        }
        "on" => {
            ctx.sandbox.lock().unwrap().enabled = true;
            colors::print_info("✓ 沙箱已开启");
        }
        "off" => {
            ctx.sandbox.lock().unwrap().enabled = false;
            colors::print_info("✓ 沙箱已关闭（不推荐）");
        }
        "save" => {
            let p = ctx.sandbox.lock().unwrap().clone();
            match p.save() {
                Ok(_) => colors::print_info("✓ 策略已写入 sandbox.json"),
                Err(e) => colors::print_error(&format!("保存失败: {e:#}")),
            }
        }
        "reload" => {
            let p = crate::sandbox::policy::SandboxPolicy::load_or_default();
            *ctx.sandbox.lock().unwrap() = p;
            colors::print_info("✓ 策略已从 sandbox.json 重载");
        }
        "test" => {
            if args.len() < 2 {
                colors::print_error("用法: /sandbox test <shell-command>");
                return Ok(CmdOutcome::Continue);
            }
            let cmd = args[1..].join(" ");
            let pol = ctx.sandbox.lock().unwrap().clone();
            let v = crate::sandbox::check::check_shell(&pol, &cmd);
            match v {
                crate::sandbox::check::SandboxVerdict::Allow => {
                    colors::print_info(&format!("✓ 允许: {cmd}"));
                }
                crate::sandbox::check::SandboxVerdict::Deny { reason } => {
                    colors::print_error(&format!("✗ 阻止: {reason}"));
                }
            }
        }
        "allow" => {
            if args.len() < 3 {
                colors::print_error("用法: /sandbox allow <read|write> <path>");
                return Ok(CmdOutcome::Continue);
            }
            let kind = args[1];
            let path = args[2].to_string();
            let mut p = ctx.sandbox.lock().unwrap();
            match kind {
                "read" => p.read_allow.push(path.clone()),
                "write" => p.write_allow.push(path.clone()),
                _ => {
                    colors::print_error("kind 必须是 read 或 write");
                    return Ok(CmdOutcome::Continue);
                }
            }
            drop(p);
            colors::print_info(&format!("✓ 已加入 {kind} allow: {path}"));
            let _ = ctx.sandbox.lock().unwrap().save();
        }
        "deny" => {
            if args.len() < 2 {
                colors::print_error("用法: /sandbox deny <shell-pattern>");
                return Ok(CmdOutcome::Continue);
            }
            let pat = args[1..].join(" ");
            ctx.sandbox.lock().unwrap().shell_deny.push(pat.clone());
            let _ = ctx.sandbox.lock().unwrap().save();
            colors::print_info(&format!("✓ 已加入 shell_deny: {pat}"));
        }
        "help" | "-h" => {
            println!();
            println!("Sandbox 命令：");
            println!();
            println!("  /sandbox status                 打印当前策略");
            println!("  /sandbox on|off                 总开关");
            println!("  /sandbox save                   写入 sandbox.json");
            println!("  /sandbox reload                 从 sandbox.json 重新加载");
            println!("  /sandbox test <cmd>             测一个 shell 命令是否允许");
            println!("  /sandbox allow <read|write> <path>   运行时加白");
            println!("  /sandbox deny <pattern>         运行时加黑");
            println!();
        }
        _ => {
            colors::print_error("用法: /sandbox [status|on|off|save|reload|test|allow|deny|help]");
        }
    }
    Ok(CmdOutcome::Continue)
}

// ─── Round 12 ─ Hermes 任务 + Cron ──────────────────────────────────

pub async fn tasks_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" | "ls" | "" => {
            let tasks = ctx.hermes.list().unwrap_or_default();
            println!();
            println!("Hermes tasks（{} 个）：", tasks.len());
            println!();
            if tasks.is_empty() {
                println!("  (空 — `/tasks add <name> shell <cmd>` 创建)");
            } else {
                for t in &tasks {
                    let cron = t.cron_expr.as_deref().unwrap_or("—");
                    let last = t
                        .last_run_at
                        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0).map(|d| d.format("%Y-%m-%d %H:%M").to_string()))
                        .unwrap_or_else(|| "—".to_string());
                    println!(
                        "  #{:<3} {:<24} [{:>9}] cron={:<14} ran={:<3} last={}",
                        t.id, t.name, t.status.as_str(), cron, t.run_count, last
                    );
                }
            }
            println!();
        }
        "show" => {
            if args.len() < 2 {
                colors::print_error("用法: /tasks show <id>");
                return Ok(CmdOutcome::Continue);
            }
            let id: i64 = match args[1].parse() {
                Ok(n) => n,
                Err(_) => {
                    colors::print_error("id 必须是整数");
                    return Ok(CmdOutcome::Continue);
                }
            };
            match ctx.hermes.get(id) {
                Ok(Some(t)) => {
                    println!();
                    println!("  id:        {}", t.id);
                    println!("  name:      {}", t.name);
                    println!("  kind:      {}", t.kind.as_str());
                    println!("  args:      {}", t.args);
                    println!("  status:    {}", t.status.as_str());
                    println!("  cron:      {}", t.cron_expr.as_deref().unwrap_or("—"));
                    println!("  run_count: {}", t.run_count);
                    println!("  error:     {}", t.error.as_deref().unwrap_or("—"));
                    println!();
                    let runs = ctx.hermes.store.runs_for(id).unwrap_or_default();
                    if !runs.is_empty() {
                        println!("  runs ({}):", runs.len());
                        for r in runs.iter().take(5) {
                            let ts = chrono::DateTime::from_timestamp(r.started_at, 0)
                                .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                                .unwrap_or_else(|| "?".to_string());
                            let dur = r
                                .finished_at
                                .map(|f| format!(" ({}s)", f - r.started_at))
                                .unwrap_or_else(|| "".to_string());
                            println!("    [{}] {}{} {}", r.status.as_str(), ts, dur, r.error.as_deref().unwrap_or(""));
                        }
                    }
                    println!();
                }
                Ok(None) => colors::print_error(&format!("task #{id} 不存在")),
                Err(e) => colors::print_error(&format!("查询失败: {e:#}")),
            }
        }
        "add" => {
            if args.len() < 3 {
                colors::print_error("用法: /tasks add <name> <kind> [args...] [--cron \"0 * * * *\"] [--auto]");
                return Ok(CmdOutcome::Continue);
            }
            let name = args[1];
            let kind = match crate::hermes::TaskKind::parse(args[2]) {
                Some(k) => k,
                None => {
                    colors::print_error(&format!("未知 kind `{}`（可选: shell / prompt / rag_query / web_search）", args[2]));
                    return Ok(CmdOutcome::Continue);
                }
            };
            // 收集 args
            let tail: Vec<String> = args[3..].iter().map(|s| s.to_string()).collect();
            // 解析 --cron / --auto 标记
            let mut cron: Option<String> = None;
            let mut auto = false;
            let mut arg_str_parts: Vec<String> = Vec::new();
            let mut i = 0;
            while i < tail.len() {
                if tail[i] == "--cron" && i + 1 < tail.len() {
                    // 取后面所有 token（直到下个 --flag 或 end）作 cron 字段
                    let mut parts = vec![tail[i + 1].clone()];
                    let mut j = i + 2;
                    while j < tail.len() && !tail[j].starts_with("--") {
                        parts.push(tail[j].clone());
                        j += 1;
                    }
                    let joined = parts.join(" ");
                    // 去首尾引号
                    let trimmed = joined.trim().trim_matches(|c| c == '"' || c == '\'');
                    cron = Some(trimmed.to_string());
                    i = j;
                    continue;
                }
                if tail[i] == "--auto" {
                    auto = true;
                    i += 1;
                    continue;
                }
                arg_str_parts.push(tail[i].clone());
                i += 1;
            }
            let args_str = if kind == crate::hermes::TaskKind::Shell {
                // shell 命令：trim 掉首尾引号
                let joined = arg_str_parts.join(" ");
                let trimmed = joined.trim();
                let trimmed = if (trimmed.starts_with('"') && trimmed.ends_with('"'))
                    || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                {
                    &trimmed[1..trimmed.len() - 1]
                } else {
                    trimmed
                };
                trimmed.to_string()
            } else {
                serde_json::to_string(&arg_str_parts).unwrap_or_else(|_| arg_str_parts.join(" "))
            };
            let approval_mode = if auto { "auto" } else { "manual" };
            match ctx.hermes.add_task(name, kind, &args_str, cron.as_deref(), approval_mode) {
                Ok(id) => {
                    if auto {
                        // auto 模式直接 approve（cron 任务免审批）
                        let _ = ctx.hermes.approve(id, "auto");
                    }
                    colors::print_info(&format!(
                        "✓ 创建 task #{id} ({name}, {}{})",
                        kind.as_str(),
                        cron.as_ref().map(|c| format!(", cron=`{c}`")).unwrap_or_default()
                    ));
                }
                Err(e) => colors::print_error(&format!("创建失败: {e:#}")),
            }
        }
        "approve" => {
            if args.len() < 2 {
                colors::print_error("用法: /tasks approve <id>");
                return Ok(CmdOutcome::Continue);
            }
            let id: i64 = match args[1].parse() {
                Ok(n) => n,
                Err(_) => {
                    colors::print_error("id 必须是整数");
                    return Ok(CmdOutcome::Continue);
                }
            };
            match ctx.hermes.approve(id, "user:manual") {
                Ok(_) => colors::print_info(&format!("✓ task #{id} 已审批")),
                Err(e) => colors::print_error(&format!("审批失败: {e:#}")),
            }
        }
        "reject" => {
            if args.len() < 2 {
                colors::print_error("用法: /tasks reject <id>");
                return Ok(CmdOutcome::Continue);
            }
            let id: i64 = match args[1].parse() {
                Ok(n) => n,
                Err(_) => {
                    colors::print_error("id 必须是整数");
                    return Ok(CmdOutcome::Continue);
                }
            };
            match ctx.hermes.reject(id) {
                Ok(_) => colors::print_info(&format!("✓ task #{id} 已拒绝")),
                Err(e) => colors::print_error(&format!("拒绝失败: {e:#}")),
            }
        }
        "run" => {
            if args.len() < 2 {
                colors::print_error("用法: /tasks run <id>");
                return Ok(CmdOutcome::Continue);
            }
            let id: i64 = match args[1].parse() {
                Ok(n) => n,
                Err(_) => {
                    colors::print_error("id 必须是整数");
                    return Ok(CmdOutcome::Continue);
                }
            };
            colors::print_info(&format!("(hermes 手动触发 #{id} ... )"));
            match ctx.hermes.run_now(id).await {
                Ok(_) => colors::print_info("✓ 完成"),
                Err(e) => colors::print_error(&format!("执行失败: {e:#}")),
            }
        }
        "delete" | "rm" => {
            if args.len() < 2 {
                colors::print_error("用法: /tasks delete <id>");
                return Ok(CmdOutcome::Continue);
            }
            let id: i64 = match args[1].parse() {
                Ok(n) => n,
                Err(_) => {
                    colors::print_error("id 必须是整数");
                    return Ok(CmdOutcome::Continue);
                }
            };
            match ctx.hermes.delete(id) {
                Ok(_) => colors::print_info(&format!("✓ task #{id} 已删除")),
                Err(e) => colors::print_error(&format!("删除失败: {e:#}")),
            }
        }
        "help" | "-h" => {
            println!();
            println!("Tasks 命令：");
            println!();
            println!("  /tasks list                     列所有 task");
            println!("  /tasks show <id>                看 task 详情 + 跑过的 run");
            println!("  /tasks add <name> <kind> [args] [--cron EXPR] [--auto]");
            println!("                                  kind: shell / prompt / rag_query / web_search");
            println!("                                  --cron: 5-field cron 表达式（周期任务）");
            println!("                                  --auto: 自动审批（cron 任务默认）");
            println!("  /tasks approve <id>             审批 pending task");
            println!("  /tasks reject <id>              拒绝 pending task");
            println!("  /tasks run <id>                 立即跑一次（无论 cron / 状态）");
            println!("  /tasks delete <id>              删除 task（+ 它所有 run）");
            println!();
        }
        _ => {
            colors::print_error("用法: /tasks [list|show|add|approve|reject|run|delete|help]");
        }
    }
    Ok(CmdOutcome::Continue)
}

pub async fn cron_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" | "ls" | "" => {
            // 列所有带 cron 表达式的 task
            let tasks: Vec<_> = ctx
                .hermes
                .list()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| t.cron_expr.is_some())
                .collect();
            println!();
            println!("Cron jobs（{} 个）：", tasks.len());
            println!();
            if tasks.is_empty() {
                println!("  (空 — 用 `/tasks add ... --cron \"0 * * * *\"` 创建)");
            } else {
                for t in tasks {
                    let next = t
                        .next_run_at
                        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0).map(|d| d.format("%Y-%m-%d %H:%M").to_string()))
                        .unwrap_or_else(|| "—".to_string());
                    println!(
                        "  #{:<3} {:<24} cron=`{:<16}` next_run={}",
                        t.id,
                        t.name,
                        t.cron_expr.as_deref().unwrap_or(""),
                        next
                    );
                }
            }
            println!();
        }
        "validate" => {
            if args.len() < 2 {
                colors::print_error("用法: /cron validate <5-field expr>");
                return Ok(CmdOutcome::Continue);
            }
            let expr = args[1..].join(" ");
            // 去掉首尾引号（heredoc 切分时可能带）
            let expr = expr.trim();
            let expr = if (expr.starts_with('"') && expr.ends_with('"'))
                || (expr.starts_with('\'') && expr.ends_with('\''))
            {
                &expr[1..expr.len() - 1]
            } else {
                expr
            };
            match crate::hermes::CronExpr::new(expr) {
                Ok(c) => {
                    let next = c.next_after(chrono::Utc::now().timestamp());
                    let next_dt = next
                        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0).map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string()))
                        .unwrap_or_else(|| "—".to_string());
                    colors::print_info(&format!("✓ 合法：{expr}"));
                    println!("  下一次触发: {next_dt}");
                }
                Err(e) => colors::print_error(&format!("✗ 无效: {e}")),
            }
        }
        "help" | "-h" => {
            println!();
            println!("Cron 命令：");
            println!();
            println!("  /cron list               列出所有带 cron 的 task");
            println!("  /cron validate <expr>    校验 + 算下一次触发");
            println!();
            println!("5-field cron 格式: 分 时 日 月 周");
            println!("  *           任意");
            println!("  5           精确");
            println!("  1,3,5       列表");
            println!("  1-5         范围");
            println!("  */5         步长");
            println!("  1-10/2      范围 + 步长");
            println!();
            println!("例子：");
            println!("  \"0 * * * *\"     每小时整点");
            println!("  \"*/15 * * * *\"   每 15 分钟");
            println!("  \"0 9-17 * * *\"  9-17 点整点（工作时间）");
            println!("  \"0 0 * * 0\"     每周日 0 点");
            println!();
        }
        _ => {
            colors::print_error("用法: /cron [list|validate|help]");
        }
    }
    Ok(CmdOutcome::Continue)
}

// ─── Round 13 ─ SOUL.md 持久身份 ───────────────────────────────────

pub async fn soul_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    let sub = args.first().copied().unwrap_or("show");
    match sub {
        "show" | "" => {
            let soul = ctx.soul.content.lock().unwrap().clone();
            println!();
            if soul.is_empty() {
                println!("SOUL 是空的。");
                println!();
                println!("  默认全局路径: {}", crate::soul::loader::global_soul_path().display());
                println!("  项目候选路径: SOUL.md / AGENTS.md（cwd 下）");
                println!();
                println!("  用 `echo 'You are a Rust expert.' >> ~/.fr_cli/soul.md` 写入。");
                println!("  或 `/soul edit` 用 $EDITOR 打开。");
                println!("  或 `/soul append <text>` 快速追加。");
            } else {
                println!("SOUL sources（{} 个）：", soul.sources.len());
                for s in &soul.sources {
                    println!("  [{}] {}", s.kind, s.path.display());
                }
                println!();
                println!("──── 合并后内容 ────");
                for line in soul.merged.lines() {
                    println!("  {line}");
                }
                println!("────────────────────");
                println!();
            }
        }
        "path" => {
            println!("全局: {}", crate::soul::loader::global_soul_path().display());
            let cwd = ctx.cwd.lock().unwrap().clone();
            for p in crate::soul::loader::project_soul_paths(&cwd) {
                println!("项目: {} {}", p.display(),
                    if p.exists() { "(已存在)" } else { "(未创建)" });
            }
        }
        "reload" => {
            let cwd = ctx.cwd.lock().unwrap().clone();
            *ctx.soul.content.lock().unwrap() = crate::soul::SoulContent::load(&cwd);
            let s = ctx.soul.content.lock().unwrap().clone();
            colors::print_info(&format!(
                "✓ SOUL 重载完成（{} 个 source，{} 字符）",
                s.sources.len(),
                s.merged.len()
            ));
        }
        "edit" => {
            let p = crate::soul::loader::global_soul_path();
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            if !p.exists() {
                // 写入默认模板
                let template = "# SOUL — 持久身份 / 价值观 / 准则\n\n\
                                ## persona\n\
                                You are a calm, deliberate coding assistant. \n\
                                回答简洁；解释为什么；不确定时反问。\n\n\
                                ## voice\n\
                                中文 / 英文混排；技术术语保留英文。\n\n\
                                ## values\n\
                                - 先想清楚再动手\n\
                                - 一次性原子（不拆碎到多个工具调用）\n\
                                - 解释 trade-off\n\n\
                                ## anti_patterns\n\
                                - 不要逐字复述用户的问题\n\
                                - 不要给虚假的承诺\n";
                std::fs::write(&p, template).ok();
                colors::print_info(&format!("✓ 已写入默认 SOUL 模板: {}", p.display()));
            }
            // 调系统 $EDITOR
            let editor = std::env::var("EDITOR")
                .or_else(|_| std::env::var("VISUAL"))
                .unwrap_or_else(|_| "vim".to_string());
            let status = std::process::Command::new(&editor)
                .arg(&p)
                .status();
            match status {
                Ok(s) if s.success() => {
                    // reload
                    let cwd = ctx.cwd.lock().unwrap().clone();
                    *ctx.soul.content.lock().unwrap() = crate::soul::SoulContent::load(&cwd);
                    ctx.heartbeat_tools.reload_policy();
                    colors::print_info("✓ SOUL 已更新并重载（heartbeat policy 同步）");
                }
                _ => colors::print_error(&format!("$EDITOR `{editor}` 退出非 0")),
            }
        }
        "append" => {
            if args.len() < 2 {
                colors::print_error("用法: /soul append <text...>");
                return Ok(CmdOutcome::Continue);
            }
            let text = args[1..].join(" ");
            let p = crate::soul::loader::global_soul_path();
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)?;
            writeln!(f, "\n{}", text)?;
            let cwd = ctx.cwd.lock().unwrap().clone();
            *ctx.soul.content.lock().unwrap() = crate::soul::SoulContent::load(&cwd);
            ctx.heartbeat_tools.reload_policy();
            colors::print_info(&format!("✓ 已追加 {} 字符到 {} (heartbeat policy 同步)", text.len(), p.display()));
        }
        "init" => {
            // 强制重写默认模板
            let p = crate::soul::loader::global_soul_path();
            if p.exists() {
                colors::print_error(&format!("SOUL 已存在: {}（用 /soul edit 改）", p.display()));
                return Ok(CmdOutcome::Continue);
            }
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let template = std::fs::read_to_string("templates/soul.md").unwrap_or_else(|_| {
                "# SOUL — 持久身份\n\n## persona\nYou are a calm, deliberate assistant.\n\n## voice\n中英混排，简洁直接。\n".to_string()
            });
            std::fs::write(&p, template).ok();
            colors::print_info(&format!("✓ 初始化 SOUL: {}", p.display()));
        }
        "help" | "-h" => {
            println!();
            println!("SOUL 命令：");
            println!();
            println!("  /soul show                  打印当前 SOUL（合并后）");
            println!("  /soul path                  打印 SOUL 文件路径");
            println!("  /soul edit                  $EDITOR 打开全局 SOUL.md");
            println!("  /soul append <text...>      快速追加一段到全局 SOUL.md");
            println!("  /soul init                  第一次初始化全局 SOUL.md");
            println!("  /soul reload                从磁盘重载（忽略缓存）");
            println!();
            println!("SOUL 多源（按优先级合并，高优先级覆盖低优先级同名 ## 段）：");
            println!("  1. ~/.fr_cli/soul.md      （全局）");
            println!("  2. <cwd>/SOUL.md          （项目级）");
            println!("  3. <cwd>/AGENTS.md        （兼容 OpenClaw / Claude Code）");
            println!();
        }
        _ => {
            colors::print_error("用法: /soul [show|path|edit|append|init|reload|help]");
        }
    }
    Ok(CmdOutcome::Continue)
}

// ─────────────────────────────────────────────────────────────
// /heartbeat ── Round 14 主动唤醒控制
// ─────────────────────────────────────────────────────────────
pub async fn heartbeat_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    // 默认行为：status
    let sub = args.first().copied().unwrap_or("status");
    match sub {
        "status" | "s" => {
            ctx.heartbeat_tools.reload_policy();
            let pol = ctx.heartbeat_tools.policy.lock().unwrap().clone();
            let st = ctx.heartbeat_tools.state.lock().unwrap().clone();
            let last_run_str = st
                .last_run_at
                .and_then(|ts| {
                    chrono::DateTime::from_timestamp(ts, 0)
                        .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                })
                .unwrap_or_else(|| "(未跑过)".to_string());
            let next_run_str = st
                .next_run_at
                .and_then(|ts| {
                    chrono::DateTime::from_timestamp(ts, 0)
                        .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                })
                .unwrap_or_else(|| "(未排期)".to_string());
            println!();
            println!("Heartbeat 状态：");
            println!("  enabled:              {}", if pol.enabled { "✓ on" } else { "✗ off" });
            println!("  interval_minutes:     {}", pol.interval_minutes);
            println!("  directives_count:     {}", pol.directives.len());
            println!("  last_run_at:          {}", last_run_str);
            println!("  next_run_at:          {}", next_run_str);
            println!("  history_count:        {}", st.history.len());
            if let Some(rep) = &st.last_report {
                let preview = rep.chars().take(120).collect::<String>();
                println!("  last_report (preview): {}", preview);
            }
            if !pol.directives.is_empty() {
                println!();
                println!("  directives:");
                for d in &pol.directives {
                    println!("    - {d}");
                }
            }
            println!();
        }
        "on" => {
            // 不 reload_policy() —— 保留 in-memory interval_minutes
            let mut pol = ctx.heartbeat_tools.policy.lock().unwrap();
            pol.enabled = true;
            let interval = pol.interval_minutes;
            drop(pol);
            // 写回 SOUL
            let soul_path = crate::soul::loader::global_soul_path();
            let pol_clone = ctx.heartbeat_tools.policy.lock().unwrap().clone();
            let _ = crate::tools::heartbeat::write_heartbeat_section_to_soul(&soul_path, &pol_clone);
            {
                let mut st = ctx.heartbeat_tools.state.lock().unwrap();
                st.enabled = true;
                if st.next_run_at.is_none() {
                    st.schedule_next(interval);
                }
                let _ = st.save();
            }
            colors::print_info(&format!(
                "✓ Heartbeat 已开启（每 {} 分钟跑一次）",
                interval
            ));
        }
        "off" => {
            let mut pol = ctx.heartbeat_tools.policy.lock().unwrap();
            pol.enabled = false;
            drop(pol);
            let soul_path = crate::soul::loader::global_soul_path();
            let pol_clone = ctx.heartbeat_tools.policy.lock().unwrap().clone();
            let _ = crate::tools::heartbeat::write_heartbeat_section_to_soul(&soul_path, &pol_clone);
            {
                let mut st = ctx.heartbeat_tools.state.lock().unwrap();
                st.enabled = false;
                let _ = st.save();
            }
            colors::print_info("✓ Heartbeat 已关闭");
        }
        "now" | "run" => {
            // 立即触发一次（async，非阻塞）
            let hb = ctx.heartbeat.clone();
            // 拿一个 Arc<AppContext> 出来：尝试从 app_ctx_factory 拿；没有就跳过
            let app = ctx
                .heartbeat_tools
                .app_ctx_factory
                .lock()
                .unwrap()
                .clone();
            let app = match app {
                Some(a) => a,
                None => {
                    colors::print_error("heartbeat app_ctx_factory 未绑定（REPL 必须先启动）");
                    return Ok(CmdOutcome::Continue);
                }
            };
            tokio::spawn(async move {
                match hb.run_now(app).await {
                    Ok(rec) => {
                        eprintln!(
                            "  [heartbeat] 手动跑完成 status={} tools={}",
                            rec.status,
                            rec.tools_called.len()
                        );
                        let preview = rec.summary.chars().take(200).collect::<String>();
                        if !preview.is_empty() {
                            eprintln!("  [heartbeat] 报告: {preview}");
                        }
                    }
                    Err(e) => eprintln!("  [heartbeat] 手动跑失败: {e:#}"),
                }
            });
            colors::print_info("✓ Heartbeat 已 spawn 到后台 task（不阻塞当前轮）");
        }
        "interval" | "i" => {
            // /heartbeat interval <N>
            if args.len() < 2 {
                colors::print_error("用法: /heartbeat interval <N>");
                return Ok(CmdOutcome::Continue);
            }
            let n: u32 = match args[1].parse() {
                Ok(n) => n,
                Err(_) => {
                    colors::print_error("interval 必须是正整数（分钟）");
                    return Ok(CmdOutcome::Continue);
                }
            };
            if n < 1 {
                colors::print_error("interval 必须 ≥ 1");
                return Ok(CmdOutcome::Continue);
            }
            let mut pol = ctx.heartbeat_tools.policy.lock().unwrap();
            pol.interval_minutes = n;
            drop(pol);
            // 写回 SOUL
            let soul_path = crate::soul::loader::global_soul_path();
            let pol_clone = ctx.heartbeat_tools.policy.lock().unwrap().clone();
            let _ = crate::tools::heartbeat::write_heartbeat_section_to_soul(&soul_path, &pol_clone);
            {
                let mut st = ctx.heartbeat_tools.state.lock().unwrap();
                st.schedule_next(n);
                let _ = st.save();
            }
            colors::print_info(&format!("✓ Heartbeat 间隔已设为 {n} 分钟"));
        }
        "directives" | "d" => {
            // 二级子命令：/heartbeat directives [list|add <text>|clear]
            let sub2 = args.get(1).copied().unwrap_or("list");
            match sub2 {
                "list" | "ls" => {
                    ctx.heartbeat_tools.reload_policy();
                    let pol = ctx.heartbeat_tools.policy.lock().unwrap();
                    if pol.directives.is_empty() {
                        println!();
                        println!("  (暂无 directives)");
                        println!("  添加: /heartbeat directives add <text...>");
                        println!();
                    } else {
                        for (i, d) in pol.directives.iter().enumerate() {
                            println!("  {}. {}", i + 1, d);
                        }
                    }
                }
                "add" => {
                    if args.len() < 3 {
                        colors::print_error("用法: /heartbeat directives add <text...>");
                        return Ok(CmdOutcome::Continue);
                    }
                    let text = args[2..].join(" ");
                    // 不 reload_policy() —— 否则会把当前 in-memory 的 enabled/interval 重置回 SOUL
                    {
                        let mut pol = ctx.heartbeat_tools.policy.lock().unwrap();
                        pol.directives.push(text.clone());
                    }
                    // 写回 SOUL.md
                    let soul_path = crate::soul::loader::global_soul_path();
                    let pol = ctx.heartbeat_tools.policy.lock().unwrap().clone();
                    let _ = crate::tools::heartbeat::write_heartbeat_section_to_soul(&soul_path, &pol);
                    colors::print_info(&format!("✓ directive 已添加并写回 SOUL: {text}"));
                }
                "clear" => {
                    {
                        let mut pol = ctx.heartbeat_tools.policy.lock().unwrap();
                        pol.directives.clear();
                    }
                    let soul_path = crate::soul::loader::global_soul_path();
                    let pol = ctx.heartbeat_tools.policy.lock().unwrap().clone();
                    let _ = crate::tools::heartbeat::write_heartbeat_section_to_soul(&soul_path, &pol);
                    colors::print_info("✓ directives 已清空并写回 SOUL");
                }
                _ => {
                    colors::print_error("用法: /heartbeat directives [list|add <text>|clear]");
                }
            }
        }
        "history" | "h" => {
            let st = ctx.heartbeat_tools.state.lock().unwrap().clone();
            if st.history.is_empty() {
                colors::print_info("(无 history)");
                return Ok(CmdOutcome::Continue);
            }
            println!();
            println!("Heartbeat history ({} runs, 最多显示 10):", st.history.len());
            for (i, r) in st.history.iter().take(10).enumerate() {
                let ts = chrono::DateTime::from_timestamp(r.ran_at, 0)
                    .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "?".to_string());
                let summary = r.summary.chars().take(80).collect::<String>();
                println!("  {}. [{}] {} (tools={}) — {}",
                    i + 1, ts, r.status, r.tools_called.len(), summary);
            }
            println!();
        }
        "reload" => {
            ctx.heartbeat_tools.reload_policy();
            let cwd = ctx.cwd.lock().unwrap().clone();
            *ctx.soul.content.lock().unwrap() = crate::soul::SoulContent::load(&cwd);
            ctx.heartbeat_tools.reload_policy();
            colors::print_info("✓ Heartbeat policy 已从 SOUL 重载");
        }
        "help" | "-h" | "?" => {
            println!();
            println!("Heartbeat 命令：");
            println!();
            println!("  /heartbeat                  status  查当前状态");
            println!("  /heartbeat on / off         开关");
            println!("  /heartbeat now              立即触发一次（async）");
            println!("  /heartbeat interval <N>     设间隔分钟数");
            println!("  /heartbeat directives       列出当前 directives");
            println!("  /heartbeat directives add <text>  添加一条 directive（写回 SOUL）");
            println!("  /heartbeat directives clear       清空所有 directives（写回 SOUL）");
            println!("  /heartbeat history          看最近 10 次跑");
            println!("  /heartbeat reload           从 SOUL 重新拉 policy");
            println!();
            println!("配置文件: SOUL.md 的 `## heartbeat` 段：");
            println!("  enabled: true|false");
            println!("  interval_minutes: <N>");
            println!("  directives:");
            println!("    - 跑 /tasks list 检查待办");
            println!("    - 跑 rag_query 复习最近学习笔记");
            println!();
        }
        _ => {
            colors::print_error("用法: /heartbeat [status|on|off|now|interval|directives|history|reload|help]");
        }
    }
    Ok(CmdOutcome::Continue)
}

// ─── Round 15c ─ PPTX 导出 ──────────────────────────────────────────

pub async fn pptx_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    use crate::export::{slides::SlideKind, Deck};

    // /pptx <sub> [args]
    // sub: session  <name>  [out.pptx]
    //      heartbeat [out.pptx]  -- 从 ~/.fr_cli/heartbeat_state.json 导
    //      demo     [out.pptx]  -- 生成一个功能演示 deck
    let sub = args.first().copied().unwrap_or("help");
    match sub {
        "session" | "s" => {
            // /pptx session <name> [out.pptx]
            if args.len() < 2 {
                colors::print_error("用法: /pptx session <name> [out.pptx]");
                return Ok(CmdOutcome::Continue);
            }
            let name = args[1];
            let out = args
                .get(2)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{}.pptx", name));
            let path = std::path::PathBuf::from(&out);
            // 读 session
            let session_path = crate::config::paths::data_dir()
                .ok()
                .map(|d| d.join("sessions").join(format!("{name}.json")));
            let Some(p) = session_path else {
                colors::print_error("找不到 data dir");
                return Ok(CmdOutcome::Continue);
            };
            if !p.exists() {
                colors::print_error(&format!("session 不存在: {}", p.display()));
                return Ok(CmdOutcome::Continue);
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                colors::print_error(&format!("读 session 失败: {}", p.display()));
                return Ok(CmdOutcome::Continue);
            };
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) else {
                colors::print_error("session json parse 失败");
                return Ok(CmdOutcome::Continue);
            };
            let messages = val
                .get("messages")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let title = format!("Session · {name}");
            let mut deck = Deck::new(&title, "fr-claw");
            deck.push(crate::export::slides::Slide {
                kind: SlideKind::title_sub(&title, format!("{} messages", messages.len())),
            });
            // 一组 slide：把 session 切成 chunk
            let mut cur: Vec<String> = Vec::new();
            let mut cur_role = String::new();
            for m in &messages {
                let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                let content = m
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if role != cur_role {
                    if !cur.is_empty() {
                        deck.push(crate::export::slides::Slide {
                            kind: SlideKind::heading(
                                format!("{} — {}", name, cur_role),
                                cur.clone(),
                            ),
                        });
                        cur.clear();
                    }
                    cur_role = role.to_string();
                }
                for line in content.lines() {
                    cur.push(line.to_string());
                    if cur.len() >= 8 {
                        deck.push(crate::export::slides::Slide {
                            kind: SlideKind::heading(
                                format!("{} — {}", name, cur_role),
                                cur.clone(),
                            ),
                        });
                        cur.clear();
                    }
                }
            }
            if !cur.is_empty() {
                deck.push(crate::export::slides::Slide {
                    kind: SlideKind::heading(
                        format!("{} — {}", name, cur_role),
                        cur,
                    ),
                });
            }
            match crate::export::write_pptx(&deck, &path) {
                Ok(()) => colors::print_info(&format!(
                    "✓ 导出 {} ({} slides) → {}",
                    name,
                    deck.slides.len(),
                    path.display()
                )),
                Err(e) => colors::print_error(&format!("export 失败: {e:#}")),
            }
        }
        "heartbeat" | "hb" => {
            let out = args
                .get(1)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "heartbeat-report.pptx".to_string());
            let path = std::path::PathBuf::from(&out);
            // 读 heartbeat state
            let state_path = crate::heartbeat::state::state_path();
            let state = crate::heartbeat::state::HeartbeatState::load();
            let mut deck = Deck::new("Heartbeat 报告", "fr-claw");
            deck.push(crate::export::slides::Slide {
                kind: SlideKind::title_sub(
                    "Heartbeat 报告",
                    format!("state at {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
                ),
            });
            let policy = ctx.heartbeat_tools.policy.lock().unwrap().clone();
            let body = vec![
                format!("enabled: {}", if policy.enabled { "✓ on" } else { "✗ off" }),
                format!("interval_minutes: {}", policy.interval_minutes),
                format!("directives_count: {}", policy.directives.len()),
                format!("history_count: {}", state.history.len()),
                format!("state_file: {}", state_path.display()),
            ];
            deck.push(crate::export::slides::Slide {
                kind: SlideKind::heading("状态", body),
            });
            if !policy.directives.is_empty() {
                deck.push(crate::export::slides::Slide {
                    kind: SlideKind::heading("Directives", policy.directives.clone()),
                });
            }
            // 最近 5 次 history
            let recent: Vec<String> = state
                .history
                .iter()
                .take(5)
                .map(|r| {
                    let ts = chrono::DateTime::from_timestamp(r.ran_at, 0)
                        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "?".to_string());
                    let summary: String = r.summary.chars().take(60).collect();
                    format!("[{}] {} — {}", ts, r.status, summary)
                })
                .collect();
            if !recent.is_empty() {
                deck.push(crate::export::slides::Slide {
                    kind: SlideKind::heading("最近 5 次 heartbeat 跑", recent),
                });
            }
            match crate::export::write_pptx(&deck, &path) {
                Ok(()) => colors::print_info(&format!(
                    "✓ 导出 → {} ({} slides)",
                    path.display(),
                    deck.slides.len()
                )),
                Err(e) => colors::print_error(&format!("export 失败: {e:#}")),
            }
        }
        "demo" | "d" => {
            // 生成一个功能演示 deck
            let out = args
                .get(1)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "fr-claw-demo.pptx".to_string());
            let path = std::path::PathBuf::from(&out);
            let mut deck = Deck::new("fr-claw 功能演示", "Mavis");
            deck.push(crate::export::slides::Slide {
                kind: SlideKind::title_sub(
                    "fr-claw",
                    "终端 AI 助手 · 凡人打字机 Rust 重制版",
                ),
            });
            deck.push(crate::export::slides::Slide {
                kind: SlideKind::section("核心能力"),
            });
            for (t, body) in [
                (
                    "对话 + 工具",
                    vec![
                        "6 个 OpenAI 兼容 provider 自动降级",
                        "ReAct agent loop (10 步 + 并行 tool)",
                        "Plan mode 提议计划 + 用户审批",
                        "Sub-agent 委派",
                    ],
                ),
                (
                    "自我进化",
                    vec![
                        "长期记忆 memorize / recall",
                        "Web 搜索自我进化 (DDoS / Brave)",
                        "Heartbeat 主动唤醒 (SOUL 驱动)",
                        "RAG 知识库 (vector + BM25 hybrid)",
                    ],
                ),
                (
                    "可扩展",
                    vec![
                        "MCP Streamable HTTP 5,700+ 工具",
                        "Hooks 4 类事件 (Pre/Post/UserPrompt/SessionStart)",
                        "Skills SKILL.md + auto trigger",
                        "Web 控制台 + Bearer Token + SSE",
                    ],
                ),
                (
                    "安全 + 编程",
                    vec![
                        "Sandbox 隔离 (路径 + 命令 + macOS exec)",
                        "Git worktree 隔离分支 + 原子 multi_edit",
                        "Hermes 后台任务 + 5-field cron",
                        "Streaming Markdown 增量渲染",
                    ],
                ),
            ] {
                deck.push(crate::export::slides::Slide {
                    kind: SlideKind::heading(t.to_string(), body.iter().map(|s| s.to_string()).collect()),
                });
            }
            deck.push(crate::export::slides::Slide {
                kind: SlideKind::section("这就是 fr-claw"),
            });
            match crate::export::write_pptx(&deck, &path) {
                Ok(()) => colors::print_info(&format!(
                    "✓ demo → {} ({} slides)",
                    path.display(),
                    deck.slides.len()
                )),
                Err(e) => colors::print_error(&format!("export 失败: {e:#}")),
            }
        }
        "help" | "-h" | _ => {
            println!();
            println!("PPTX 命令：");
            println!();
            println!("  /pptx session <name> [out.pptx]   把 session 导出 PPT");
            println!("  /pptx heartbeat [out.pptx]        导出 heartbeat 状态报告");
            println!("  /pptx demo [out.pptx]             生成功能演示 deck");
            println!();
        }
    }
    Ok(CmdOutcome::Continue)
}

// ─── Round 15d ─ TTS (macOS) ──────────────────────────────────────────

pub async fn tts_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    use crate::tts::{synthesize, list_voices, TtsFormat, TtsOptions};

    let sub = args.first().copied().unwrap_or("help");
    match sub {
        "voices" | "v" => {
            match list_voices() {
                Ok(vs) if vs.is_empty() => colors::print_info("(本平台无 voice 列表)"),
                Ok(vs) => {
                    println!();
                    println!("可用 voice（{} 个）：", vs.len());
                    for v in vs.iter().take(40) {
                        println!("  {v}");
                    }
                    if vs.len() > 40 {
                        println!("  ... ({} more)", vs.len() - 40);
                    }
                    println!();
                }
                Err(e) => colors::print_error(&format!("list_voices 失败: {e}")),
            }
        }
        "say" | "s" => {
            // /tts say <text...> [out.m4a] [--voice X] [--rate N] [--aiff]
            if args.len() < 2 {
                colors::print_error("用法: /tts say <text...> [out.m4a] [--voice X] [--rate N] [--aiff]");
                return Ok(CmdOutcome::Continue);
            }
            let mut voice: Option<String> = None;
            let mut rate: Option<u32> = None;
            let mut format = TtsFormat::M4a;
            let mut text_parts: Vec<&str> = Vec::new();
            for a in &args[1..] {
                match *a {
                    "--aiff" => format = TtsFormat::Aiff,
                    _ if a.starts_with("--voice=") => {
                        voice = a.strip_prefix("--voice=").map(|s| s.to_string());
                    }
                    _ if a.starts_with("--rate=") => {
                        if let Some(n) = a.strip_prefix("--rate=") {
                            rate = n.parse().ok();
                        }
                    }
                    _ => text_parts.push(a),
                }
            }
            let text = text_parts.join(" ");
            if text.is_empty() {
                colors::print_error("text 不能为空");
                return Ok(CmdOutcome::Continue);
            }
            // 最后一个非 flag 单词如果不是以 .m4a / .aiff 结尾，就当 out
            let mut out: Option<String> = None;
            if let Some(last) = text_parts.last() {
                if last.ends_with(".m4a") || last.ends_with(".aiff") {
                    out = Some(last.to_string());
                    text_parts.pop();
                }
            }
            let out = out.unwrap_or_else(|| {
                let ext = match format {
                    TtsFormat::Aiff => "aiff",
                    TtsFormat::M4a => "m4a",
                };
                format!("tts-{}.{}", chrono::Local::now().format("%Y%m%d-%H%M%S"), ext)
            });
            let text = text_parts.join(" ");

            let opts = TtsOptions {
                voice: voice.clone(),
                rate,
                format: format.clone(),
            };
            match synthesize(&text, &out, &opts) {
                Ok(()) => {
                    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                    colors::print_info(&format!(
                        "✓ TTS → {} ({} bytes, format={:?}{})",
                        out,
                        size,
                        format,
                        voice
                            .as_deref()
                            .map(|v| format!(", voice={v}"))
                            .unwrap_or_default()
                    ));
                }
                Err(e) => colors::print_error(&format!("synthesize 失败: {e:#}")),
            }
        }
        "session" | "sess" => {
            // /tts session <name> [out.m4a]  -- 读 session 的 assistant 消息，逐条 TTS
            if args.len() < 2 {
                colors::print_error("用法: /tts session <name> [out.m4a]");
                return Ok(CmdOutcome::Continue);
            }
            let name = args[1];
            let session_path = crate::config::paths::data_dir()
                .ok()
                .map(|d| d.join("sessions").join(format!("{name}.json")));
            let Some(p) = session_path else {
                colors::print_error("找不到 data dir");
                return Ok(CmdOutcome::Continue);
            };
            if !p.exists() {
                colors::print_error(&format!("session 不存在: {}", p.display()));
                return Ok(CmdOutcome::Continue);
            }
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            let val: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            let messages = val.get("messages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            // 收集所有 assistant content
            let collected: String = messages
                .iter()
                .filter_map(|m| {
                    let role = m.get("role").and_then(|v| v.as_str())?;
                    if role != "assistant" { return None; }
                    m.get("content").and_then(|v| v.as_str()).map(|s| s.to_string())
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            if collected.is_empty() {
                colors::print_info("(session 没有 assistant 消息)");
                return Ok(CmdOutcome::Continue);
            }
            // 截断长文本（say 在巨长文本上会卡住）
            let truncated: String = if collected.chars().count() > 8000 {
                let s: String = collected.chars().take(8000).collect();
                format!("{s}\n[截断]")
            } else {
                collected
            };
            let out = args
                .get(2)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{name}-tts.m4a"));
            let opts = TtsOptions::default();
            match synthesize(&truncated, &out, &opts) {
                Ok(()) => {
                    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                    colors::print_info(&format!(
                        "✓ session TTS → {} ({} bytes, {} chars)",
                        out,
                        size,
                        truncated.chars().count()
                    ));
                }
                Err(e) => colors::print_error(&format!("synthesize 失败: {e:#}")),
            }
        }
        "help" | "-h" | _ => {
            println!();
            println!("TTS 命令（macOS only — 用 `say` + `afconvert`）：");
            println!();
            println!("  /tts voices                              列出可用 voice");
            println!("  /tts say <text...> [out.m4a]             合成一段文字");
            println!("    --voice=<name>                          指定 voice（如 Tingting / Sin-ji）");
            println!("    --rate=<wpm>                            速率（默认 175）");
            println!("    --aiff                                  出 AIFF（不转 m4a）");
            println!("  /tts session <name> [out.m4a]            把 session 助理消息合成");
            println!();
            // 忽略 ctx
            let _ = ctx;
        }
    }
    Ok(CmdOutcome::Continue)
}

// ─── Round 16 ─ 多通讯通道（飞书/钉钉/企微/Webhook） ──────────────────────

pub async fn channels_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    use crate::channels::{config::{ChannelConfig, ChannelKind, ChannelsFile}, OutboundMessage};

    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" | "ls" | "" => {
            let names = ctx.channels.list();
            println!();
            if names.is_empty() {
                println!("Channels: (空) — /channels add 配一个");
            } else {
                println!("Channels（{} 个）：", names.len());
                for n in &names {
                    let ch = ctx.channels.get(n).unwrap();
                    println!("  • {:<24}  kind={:?}", n, ch.kind());
                }
            }
            println!();
            println!("  配置文件: {}", ChannelsFile::path().display());
            println!();
        }
        "path" => println!("{}", ChannelsFile::path().display()),
        "add" => {
            // /channels add <name> <kind> <webhook_url> [secret]
            if args.len() < 4 {
                colors::print_error("用法: /channels add <name> <lark|dingtalk|wecom|webhook> <webhook_url> [secret]");
                return Ok(CmdOutcome::Continue);
            }
            let name = args[1].to_string();
            let kind = match args[2] {
                "lark" => ChannelKind::Lark,
                "dingtalk" | "dd" => ChannelKind::Dingtalk,
                "wecom" | "wechat" => ChannelKind::Wecom,
                "webhook" | "generic" => ChannelKind::Webhook,
                _ => {
                    colors::print_error(&format!("未知 kind: {}", args[2]));
                    return Ok(CmdOutcome::Continue);
                }
            };
            let url = args[3].to_string();
            let secret = args.get(4).map(|s| s.to_string());
            let mut cfg = ChannelsFile::load_or_default();
            // 覆盖同名的
            cfg.channels.retain(|c| c.name != name);
            cfg.channels.push(ChannelConfig {
                name,
                kind,
                webhook_url: url,
                secret,
                enabled: true,
                dry_run: true, // 默认 dry_run=on，用户需手动 /channels toggle 关
            });
            if let Err(e) = cfg.save() {
                colors::print_error(&format!("保存失败: {e}"));
                return Ok(CmdOutcome::Continue);
            }
            colors::print_info(&format!(
                "✓ 已添加 (dry_run=true) — 调 /channels test <name> <text> 试发，调 /channels toggle <name> 关 dry-run"
            ));
        }
        "remove" | "rm" => {
            if args.len() < 2 {
                colors::print_error("用法: /channels remove <name>");
                return Ok(CmdOutcome::Continue);
            }
            let name = args[1];
            let mut cfg = ChannelsFile::load_or_default();
            let before = cfg.channels.len();
            cfg.channels.retain(|c| c.name != name);
            if cfg.channels.len() == before {
                colors::print_error(&format!("channel `{name}` 不存在"));
                return Ok(CmdOutcome::Continue);
            }
            let _ = cfg.save();
            colors::print_info(&format!("✓ 删除 {name}"));
        }
        "toggle" => {
            if args.len() < 2 {
                colors::print_error("用法: /channels toggle <name>  (切换 dry_run)");
                return Ok(CmdOutcome::Continue);
            }
            let name = args[1];
            let mut cfg = ChannelsFile::load_or_default();
            let mut found = false;
            for c in &mut cfg.channels {
                if c.name == name {
                    c.dry_run = !c.dry_run;
                    found = true;
                    colors::print_info(&format!(
                        "✓ {name} dry_run = {}",
                        if c.dry_run { "true" } else { "false" }
                    ));
                }
            }
            if !found {
                colors::print_error(&format!("channel `{name}` 不存在"));
                return Ok(CmdOutcome::Continue);
            }
            let _ = cfg.save();
        }
        "test" | "send" => {
            // /channels test <name> <text...>
            if args.len() < 3 {
                colors::print_error("用法: /channels test <name> <text...>");
                return Ok(CmdOutcome::Continue);
            }
            let name = args[1];
            let text = args[2..].join(" ");
            let msg = OutboundMessage::text(&text);
            match ctx.channels.send(name, &msg).await {
                Ok(r) if r.ok => {
                    colors::print_info(&format!(
                        "✓ {} 发成功：{}",
                        r.channel,
                        serde_json::to_string_pretty(
                            r.platform_response.as_ref().unwrap_or(&serde_json::json!({}))
                        )
                        .unwrap_or_default()
                    ));
                }
                Ok(r) => colors::print_error(&format!(
                    "✗ {} 发失败：{}",
                    r.channel,
                    r.error.unwrap_or_else(|| "未知".into())
                )),
                Err(e) => colors::print_error(&format!("✗ {e:#}")),
            }
        }
        "broadcast" => {
            if args.len() < 2 {
                colors::print_error("用法: /channels broadcast <text...>");
                return Ok(CmdOutcome::Continue);
            }
            let text = args[1..].join(" ");
            let msg = OutboundMessage::text(&text);
            let results = ctx.channels.broadcast(&msg).await;
            for r in results {
                if r.ok {
                    colors::print_info(&format!("✓ {} 成功", r.channel));
                } else {
                    colors::print_error(&format!(
                        "✗ {} 失败：{}",
                        r.channel,
                        r.error.unwrap_or_else(|| "未知".into())
                    ));
                }
            }
        }
        "init" => {
            // /channels init ── 创建示例 channels.json（带 4 个 dry_run=true 的演示 channel）
            let path = ChannelsFile::path();
            if path.exists() {
                colors::print_error(&format!("已存在: {}", path.display()));
                return Ok(CmdOutcome::Continue);
            }
            let sample = ChannelsFile {
                channels: vec![
                    ChannelConfig {
                        name: "lark-demo".into(),
                        kind: ChannelKind::Lark,
                        webhook_url: "https://open.feishu.cn/open-apis/bot/v2/hook/<YOUR_TOKEN>".into(),
                        secret: Some("<YOUR_SECRET>".into()),
                        enabled: true,
                        dry_run: true,
                    },
                    ChannelConfig {
                        name: "dingtalk-demo".into(),
                        kind: ChannelKind::Dingtalk,
                        webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=<YOUR_TOKEN>".into(),
                        secret: Some("<YOUR_SECRET>".into()),
                        enabled: true,
                        dry_run: true,
                    },
                    ChannelConfig {
                        name: "wecom-demo".into(),
                        kind: ChannelKind::Wecom,
                        webhook_url: "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=<YOUR_KEY>".into(),
                        secret: None,
                        enabled: true,
                        dry_run: true,
                    },
                    ChannelConfig {
                        name: "n8n-webhook".into(),
                        kind: ChannelKind::Webhook,
                        webhook_url: "https://your-n8n.example.com/webhook/fr".into(),
                        secret: None,
                        enabled: true,
                        dry_run: true,
                    },
                ],
            };
            sample.save().map_err(|e| anyhow::anyhow!("保存失败: {e}"))?;
            colors::print_info(&format!("✓ 已生成示例 {} （全部 dry_run=true）", path.display()));
            colors::print_info("  1. 改 webhook_url + secret");
            colors::print_info("  2. /channels toggle <name>  取消 dry-run");
            colors::print_info("  3. /channels test <name> 你好");
        }
        "help" | "-h" | _ => {
            println!();
            println!("Channels 命令：");
            println!();
            println!("  /channels list                列出已配 channel");
            println!("  /channels add <name> <kind> <url> [secret]  加 channel");
            println!("    kind: lark | dingtalk | wecom | webhook");
            println!("  /channels remove <name>       删");
            println!("  /channels toggle <name>       切 dry_run");
            println!("  /channels test <name> <text>  试发一条");
            println!("  /channels broadcast <text>    广播到所有 channel");
            println!("  /channels init                生成示例 channels.json");
            println!();
        }
    }
    Ok(CmdOutcome::Continue)
}

// ─── Round 16e ─ Timeline HTML 导出 ────────────────────────────────────

pub async fn timeline_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    use crate::export::timeline_html::{render_session_json, TimelineOptions};

    // /timeline session <name> [out.html] [--theme auto|dark|light]
    // /timeline demo [out.html]
    let sub = args.first().copied().unwrap_or("help");
    match sub {
        "session" | "s" => {
            if args.len() < 2 {
                colors::print_error("用法: /timeline session <name> [out.html] [--theme dark|light|auto]");
                return Ok(CmdOutcome::Continue);
            }
            let name = args[1];
            let mut out: Option<String> = None;
            let mut theme = "auto".to_string();
            for a in &args[2..] {
                if let Some(t) = a.strip_prefix("--theme=") {
                    theme = t.to_string();
                } else if a.ends_with(".html") {
                    out = Some(a.to_string());
                }
            }
            let out = out.unwrap_or_else(|| format!("{name}.html"));
            let session_path = crate::config::paths::data_dir()
                .ok()
                .map(|d| d.join("sessions").join(format!("{name}.json")));
            let Some(p) = session_path else {
                colors::print_error("找不到 data dir");
                return Ok(CmdOutcome::Continue);
            };
            if !p.exists() {
                colors::print_error(&format!("session 不存在: {}", p.display()));
                return Ok(CmdOutcome::Continue);
            }
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            let opts = TimelineOptions {
                title: format!("Session · {name}"),
                author: "fr-claw".into(),
                theme: theme.clone(),
                show_timestamps: true,
            };
            let out_path = std::path::PathBuf::from(&out);
            match render_session_json(&text, &opts, &out_path) {
                Ok(()) => {
                    let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
                    colors::print_info(&format!(
                        "✓ Timeline → {} ({} bytes, theme={})",
                        out_path.display(),
                        size,
                        theme
                    ));
                }
                Err(e) => colors::print_error(&format!("render 失败: {e:#}")),
            }
        }
        "demo" | "d" => {
            // 构造一个 demo session
            let mut out: Option<String> = None;
            let mut theme = "auto".to_string();
            for a in &args[1..] {
                if let Some(t) = a.strip_prefix("--theme=") {
                    theme = t.to_string();
                } else if a.ends_with(".html") {
                    out = Some(a.to_string());
                }
            }
            let out = out.unwrap_or_else(|| "timeline-demo.html".to_string());
            let out_path = std::path::PathBuf::from(&out);
            let sample = serde_json::json!({
                "name": "demo",
                "messages": [
                    { "role": "user", "content": "# 你好 fr-claw\n\n帮我做个**Hello World**" },
                    { "role": "assistant", "content": "你好！下面是一个 Rust 的 Hello World：\n\n```rust\nfn main() {\n    println!(\"Hello, fr-claw!\");\n}\n```\n\n- 编译：`rustc main.rs`\n- 运行：`./main`" },
                    { "role": "user", "content": "再解释下 `println!` 里那个 `!` 是啥意思" },
                    { "role": "assistant", "content": "**`println!`** 是 Rust 的**宏**（macro），不是普通函数。\n\n宏用 `!` 后缀区分，编译期展开，**零运行时开销**。\n\n普通函数调用：\n```rust\nfn add(a: i32, b: i32) -> i32 { a + b }\n```\n宏调用：\n```rust\nprintln!(\"a + b = {}\", add(1, 2));\n```" },
                ]
            });
            let opts = TimelineOptions {
                title: "fr-claw Timeline Demo".into(),
                author: "fr-claw".into(),
                theme: theme.clone(),
                show_timestamps: true,
            };
            let messages = sample.get("messages").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            match crate::export::timeline_html::render_messages(&messages, &opts, &out_path) {
                Ok(()) => {
                    let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
                    colors::print_info(&format!(
                        "✓ Demo → {} ({} bytes, theme={})",
                        out_path.display(),
                        size,
                        theme
                    ));
                }
                Err(e) => colors::print_error(&format!("render 失败: {e:#}")),
            }
        }
        "help" | "-h" | _ => {
            println!();
            println!("Timeline HTML 命令：");
            println!();
            println!("  /timeline session <name> [out.html] [--theme dark|light|auto]");
            println!("  /timeline demo [out.html] [--theme dark|light|auto]");
            println!();
            println!("特点：自包含 HTML（无外链）、暗/亮色自适应、时间线布局、点 card 复制内容");
            println!();
            let _ = ctx;
        }
    }
    Ok(CmdOutcome::Continue)
}

// ─── Round 16f ─ Voice 录制 + STT ──────────────────────────────────────

pub async fn voice_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    use crate::voice::{check_env, record, record_and_transcribe, transcribe, AudioFormat, SttOptions};

    let sub = args.first().copied().unwrap_or("help");
    match sub {
        "check" | "env" => {
            let _ = ctx;
            match check_env() {
                Ok(c) => {
                    println!();
                    println!("Voice 环境检查：");
                    println!("  OPENAI_API_KEY:  {}", if c.api_key_present { "✓" } else { "✗ 未设置" });
                    println!("  sox:             {}", if c.has_sox { "✓" } else { "✗" });
                    println!("  rec:             {}", if c.has_rec { "✓" } else { "✗" });
                    println!("  ffmpeg:          {}", if c.has_ffmpeg { "✓" } else { "✗" });
                    println!();
                    if !c.api_key_present {
                        colors::print_info("  设置: export OPENAI_API_KEY=sk-...");
                    }
                    if !c.has_recorder {
                        colors::print_info("  装录音工具: brew install sox");
                    }
                    println!();
                }
                Err(e) => colors::print_error(&format!("{e}")),
            }
        }
        "record" | "r" => {
            // /voice record [out.wav] [--duration 30] [--m4a]
            let mut out: Option<String> = None;
            let mut duration = 30u32;
            let mut format = AudioFormat::Wav;
            for a in &args[1..] {
                if let Some(d) = a.strip_prefix("--duration=") {
                    if let Ok(n) = d.parse::<u32>() { duration = n; }
                } else if *a == "--m4a" {
                    format = AudioFormat::M4a;
                } else if a.ends_with(".wav") || a.ends_with(".m4a") {
                    out = Some(a.to_string());
                }
            }
            let ext = format.extension();
            let out = out.unwrap_or_else(|| {
                let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
                format!("voice-{ts}.{ext}")
            });
            let opts = SttOptions { duration_secs: duration, format, ..Default::default() };
            match record(&out, &opts) {
                Ok(p) => {
                    let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                    colors::print_info(&format!("✓ 录音完成 → {} ({} bytes, {}s)", p.display(), size, duration));
                }
                Err(e) => colors::print_error(&format!("录音失败: {e:#}")),
            }
        }
        "transcribe" | "t" => {
            // /voice transcribe <audio.wav> [--lang zh]
            if args.len() < 2 {
                colors::print_error("用法: /voice transcribe <audio.wav> [--lang zh|en]");
                return Ok(CmdOutcome::Continue);
            }
            let path = args[1];
            let mut language = None;
            for a in &args[2..] {
                if let Some(l) = a.strip_prefix("--lang=") {
                    language = Some(l.to_string());
                }
            }
            let opts = SttOptions { language, ..Default::default() };
            match transcribe(path, &opts).await {
                Ok(r) => {
                    println!();
                    println!("  📝 {}", r.text);
                    println!();
                    colors::print_info(&format!(
                        "  (model={}, size={} bytes)",
                        r.model, r.audio_size
                    ));
                }
                Err(e) => colors::print_error(&format!("转写失败: {e:#}")),
            }
        }
        "say" | "s" => {
            // /voice say [out.wav] [--duration 30] [--lang zh]   一站式录音+转写
            let mut out_dir = std::env::temp_dir().join("fr-voice");
            let mut duration = 30u32;
            let mut format = AudioFormat::Wav;
            let mut language = None;
            for a in &args[1..] {
                if let Some(d) = a.strip_prefix("--duration=") {
                    if let Ok(n) = d.parse::<u32>() { duration = n; }
                } else if let Some(l) = a.strip_prefix("--lang=") {
                    language = Some(l.to_string());
                } else if *a == "--m4a" {
                    format = AudioFormat::M4a;
                } else if !a.starts_with("--") {
                    out_dir = std::path::PathBuf::from(a);
                }
            }
            let opts = SttOptions { duration_secs: duration, format, language, ..Default::default() };
            colors::print_info(&format!("🎙  录音中... ({}s)", duration));
            match record_and_transcribe(&out_dir, &opts).await {
                Ok(r) => {
                    println!();
                    println!("  📝 {}", r.text);
                    println!();
                    colors::print_info(&format!(
                        "  (model={}, audio={}, size={} bytes)",
                        r.model, r.audio_file, r.audio_size
                    ));
                }
                Err(e) => colors::print_error(&format!("录音+转写失败: {e:#}")),
            }
        }
        "help" | "-h" | _ => {
            let _ = ctx;
            println!();
            println!("Voice 命令（macOS / Linux —— 需 sox 或 ffmpeg + OPENAI_API_KEY）：");
            println!();
            println!("  /voice check                  环境检查（API key / 录音工具）");
            println!("  /voice record [out.wav] [--duration 30] [--m4a]");
            println!("                                录音到文件（不转写）");
            println!("  /voice transcribe <file> [--lang zh|en]");
            println!("                                已有音频 → Whisper 转写");
            println!("  /voice say [out_dir] [--duration 30] [--lang zh] [--m4a]");
            println!("                                一站式：录音 + Whisper 转写");
            println!();
            println!("环境：");
            println!("  OPENAI_API_KEY                Whisper API key (必填)");
            println!("  OPENAI_BASE_URL               可选，OpenAI 兼容 base url");
            println!("  WHISPER_MODEL                 可选，默认 whisper-1");
            println!();
        }
    }
    Ok(CmdOutcome::Continue)
}