//! 交互式配置命令 (`/config`)。
//!
//! 用法：
//!   /config              进入配置菜单
//!   /config model        直接启动模型 provider 向导
//!   /config channel      直接启动通讯通道向导
//!   /config mode         直接启动思考模式向导
//!   /config lang         直接启动语言向导
//!   /config autonomous   直接启动自治模式向导
//!   /config limit        直接启动单轮限制向导

use crate::agent::ThinkingMode;
use crate::channels::config::{ChannelConfig, ChannelKind, ChannelsFile};
use crate::channels::ChannelManager;
use crate::config::keys;
use crate::config::models::{self, ProviderConfig};
use crate::config::paths;
use crate::llm::registry::FallbackChain;
use crate::repl::command::CmdOutcome;
use crate::repl::context::AppContext;
use anyhow::Result;
use std::collections::BTreeMap;
use std::io::{self, Write};

/// 预设 provider：alias、显示名、默认 base_url、常见模型列表。
struct ProviderPreset {
    alias: &'static str,
    name: &'static str,
    base_url: &'static str,
    api_key_env: &'static str,
    models: &'static [&'static str],
}

const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        alias: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        api_key_env: "OPENAI_API_KEY",
        models: &["gpt-4o", "gpt-4o-mini", "o3-mini", "o1-mini"],
    },
    ProviderPreset {
        alias: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        api_key_env: "DEEPSEEK_API_KEY",
        models: &["deepseek-chat", "deepseek-reasoner"],
    },
    ProviderPreset {
        alias: "anthropic",
        name: "Anthropic Claude",
        base_url: "https://api.anthropic.com/v1",
        api_key_env: "ANTHROPIC_API_KEY",
        models: &[
            "claude-3-5-sonnet-20241022",
            "claude-3-opus-20240229",
            "claude-3-haiku-20240307",
        ],
    },
    ProviderPreset {
        alias: "moonshot",
        name: "Moonshot Kimi",
        base_url: "https://api.moonshot.cn/v1",
        api_key_env: "MOONSHOT_API_KEY",
        models: &["moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"],
    },
    ProviderPreset {
        alias: "zhipu",
        name: "智谱 GLM",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        api_key_env: "ZHIPU_API_KEY",
        models: &["glm-4-flash", "glm-4", "glm-4v", "glm-4-air"],
    },
    ProviderPreset {
        alias: "ollama",
        name: "Ollama (本地)",
        base_url: "http://127.0.0.1:11434/v1",
        api_key_env: "OLLAMA_API_KEY",
        models: &["qwen2.5:7b", "llama3.2", "phi4", "deepseek-r1:7b"],
    },
];

pub async fn config_cmd(ctx: &AppContext, args: &[&str]) -> Result<CmdOutcome> {
    if args.is_empty() {
        show_menu();
        return run_menu(ctx).await;
    }

    match args[0] {
        "model" | "m" => configure_model(ctx).await,
        "channel" | "channels" | "ch" => configure_channel(ctx).await,
        "mode" | "thinking" => configure_thinking_mode(ctx).await,
        "lang" | "language" => configure_lang(ctx).await,
        "autonomous" | "auto" => configure_autonomous(ctx).await,
        "limit" => configure_limit(ctx).await,
        other => {
            crate::ui::colors::print_error(&format!("未知配置项: {other}"));
            show_menu();
            Ok(CmdOutcome::Continue)
        }
    }
}

fn show_menu() {
    println!();
    println!("/config 交互式配置");
    println!("-------------------");
    println!("  1) 模型       (model)      配置或添加 LLM provider");
    println!("  2) 通道       (channel)    配置通知通道（飞书/钉钉/企微/Webhook）");
    println!("  3) 思考模式   (mode)       切换思维模式");
    println!("  4) 语言       (lang)       切换界面与 system prompt 语言");
    println!("  5) 自治模式   (autonomous) 切换工具授权自治模式");
    println!("  6) 单轮限制   (limit)      设置单轮 max_tokens");
    println!("  0) 退出");
    println!();
}

async fn run_menu(ctx: &AppContext) -> Result<CmdOutcome> {
    let choice = match prompt_choice("请选择配置项", 6)? {
        Some(c) => c,
        None => return Ok(CmdOutcome::Continue),
    };

    match choice {
        1 => configure_model(ctx).await,
        2 => configure_channel(ctx).await,
        3 => configure_thinking_mode(ctx).await,
        4 => configure_lang(ctx).await,
        5 => configure_autonomous(ctx).await,
        6 => configure_limit(ctx).await,
        _ => Ok(CmdOutcome::Continue),
    }
}

// ---------------- 模型 provider 向导 ----------------

async fn configure_model(ctx: &AppContext) -> Result<CmdOutcome> {
    println!();
    println!("配置模型 provider");
    println!("-----------------");

    // 1. 选择 provider 预设或自定义
    println!("请选择模型提供商：");
    for (i, p) in PRESETS.iter().enumerate() {
        println!("  {}) {}", i + 1, p.name);
    }
    println!("  {}) 自定义", PRESETS.len() + 1);

    let choice = match prompt_choice("输入编号", PRESETS.len() + 1)? {
        Some(c) => c,
        None => return Ok(CmdOutcome::Continue),
    };

    let (alias, name, base_url, api_key_env, model) = if choice <= PRESETS.len() {
        let preset = &PRESETS[choice - 1];

        // 2. 选择模型
        println!();
        println!("请选择 {} 模型：", preset.name);
        for (i, m) in preset.models.iter().enumerate() {
            println!("  {}) {}", i + 1, m);
        }
        println!("  {}) 自定义", preset.models.len() + 1);

        let model_choice = match prompt_choice("输入编号", preset.models.len() + 1)? {
            Some(c) => c,
            None => return Ok(CmdOutcome::Continue),
        };

        let model = if model_choice <= preset.models.len() {
            preset.models[model_choice - 1].to_string()
        } else {
            prompt_line("输入模型名: ")?.trim().to_string()
        };

        // 3. 确认 base_url
        println!();
        let input = prompt_line(&format!("base_url [{}]: ", preset.base_url))?;
        let base_url = if input.trim().is_empty() {
            preset.base_url.to_string()
        } else {
            input.trim().to_string()
        };

        (
            preset.alias.to_string(),
            preset.name.to_string(),
            base_url,
            Some(preset.api_key_env.to_string()),
            model,
        )
    } else {
        // 自定义 provider
        let alias = loop {
            let v = prompt_line("provider alias (唯一标识，如 my-openai): ")?;
            let v = v.trim().to_string();
            if !v.is_empty() {
                break v;
            }
            crate::ui::colors::print_error("alias 不能为空");
        };

        let name = prompt_line("显示名称: ")?;
        let name = name.trim().to_string();
        let name = if name.is_empty() { alias.clone() } else { name };

        let base_url = loop {
            let v = prompt_line("base_url: ")?;
            let v = v.trim().to_string();
            if !v.is_empty() {
                break v;
            }
            crate::ui::colors::print_error("base_url 不能为空");
        };

        let model = loop {
            let v = prompt_line("模型名: ")?;
            let v = v.trim().to_string();
            if !v.is_empty() {
                break v;
            }
            crate::ui::colors::print_error("模型名 不能为空");
        };

        let api_key_env = prompt_line("API key 环境变量名 (可直接回车跳过): ")?;
        let api_key_env = {
            let s = api_key_env.trim();
            if s.is_empty() { None } else { Some(s.to_string()) }
        };

        (alias, name, base_url, api_key_env, model)
    };

    // 4. 输入 api key
    println!();
    let api_key = prompt_sensitive("API key (输入不会显示): ")?;
    let api_key = api_key.trim().to_string();

    // 5. 是否设为 default
    let set_default = prompt_yes_no("是否设为默认 provider", true)?;

    // 保存 provider 配置到 models.yaml
    {
        let mut models = ctx.models.lock().unwrap();

        if set_default {
            for p in models.providers.values_mut() {
                p.is_default = false;
            }
            models.settings.default_provider = Some(alias.clone());
        }

        models.providers.insert(
            alias.clone(),
            ProviderConfig {
                name,
                model,
                protocol: "openai".to_string(),
                base_url,
                api_key_env,
                max_tokens: Some(8192),
                temperature: Some(0.7),
                is_default: set_default,
                is_backup: false,
                extra_headers: BTreeMap::new(),
            },
        );

        let path = paths::models_yaml_path()?;
        models::write_to(&path, &models)?;
    }

    // 保存 api key 到 keys.json
    if !api_key.is_empty() {
        keys::set_key(&alias, &api_key)?;
    }

    // 重建并更新 provider 链，使新配置在当前会话立即生效
    {
        let models_guard = ctx.models.lock().unwrap();
        let new_chain = FallbackChain::from_models(&models_guard)?;
        *ctx.chain.write().unwrap() = new_chain;
    }

    // 同步当前会话
    {
        let mut session = ctx.session.lock().unwrap();
        session.provider_alias = alias.clone();
    }

    crate::ui::colors::print_info(&format!("已保存 provider `{alias}` 并切换到该模型"));
    println!("  配置文件: ~/.fr_cli/models.yaml");
    if !api_key.is_empty() {
        println!("  API key:  ~/.fr_cli/keys.json");
    }
    println!();

    Ok(CmdOutcome::Continue)
}

// ---------------- 通讯通道向导 ----------------

async fn configure_channel(ctx: &AppContext) -> Result<CmdOutcome> {
    use crate::ui::colors;

    println!();
    println!("配置通知通道");
    println!("-------------");

    println!("请选择通道类型：");
    println!("  1) 飞书 (lark)");
    println!("  2) 钉钉 (dingtalk)");
    println!("  3) 企业微信 (wecom)");
    println!("  4) 通用 Webhook (webhook)");

    let kind = match prompt_choice("输入编号", 4)? {
        Some(1) => ChannelKind::Lark,
        Some(2) => ChannelKind::Dingtalk,
        Some(3) => ChannelKind::Wecom,
        Some(4) => ChannelKind::Webhook,
        _ => return Ok(CmdOutcome::Continue),
    };

    let name = loop {
        let v = prompt_line("通道名称 (唯一标识): ")?;
        let v = v.trim().to_string();
        if !v.is_empty() {
            break v;
        }
        colors::print_error("名称不能为空");
    };

    let webhook_url = loop {
        let v = prompt_line("webhook URL: ")?;
        let v = v.trim().to_string();
        if !v.is_empty() {
            break v;
        }
        colors::print_error("webhook URL 不能为空");
    };

    let secret = prompt_line("secret/加签密钥 (无则回车跳过): ")?;
    let secret = {
        let s = secret.trim();
        if s.is_empty() { None } else { Some(s.to_string()) }
    };

    let dry_run = prompt_yes_no("是否开启 dry-run（只模拟发送，不真发）", true)?;
    let enabled = prompt_yes_no("是否立即启用", true)?;

    let mut cfg = ChannelsFile::load_or_default();
    cfg.channels.retain(|c| c.name != name);
    cfg.channels.push(ChannelConfig {
        name: name.clone(),
        kind,
        webhook_url,
        secret,
        enabled,
        dry_run,
    });
    cfg.save()?;

    // 热重载内存中的 ChannelManager
    {
        let new_manager = ChannelManager::from_config(&cfg);
        *ctx.channels.write().unwrap() = new_manager;
    }

    colors::print_info(&format!("✓ 已保存通道 `{name}` 并热重载"));
    println!("  配置文件: {}", ChannelsFile::path().display());
    println!();
    Ok(CmdOutcome::Continue)
}

// ---------------- 思考模式向导 ----------------

async fn configure_thinking_mode(ctx: &AppContext) -> Result<CmdOutcome> {
    use crate::ui::colors;

    println!();
    println!("配置思考模式");
    println!("-------------");

    let modes = [
        ThinkingMode::Direct,
        ThinkingMode::CoT,
        ThinkingMode::ToT,
        ThinkingMode::ReAct,
        ThinkingMode::Plan,
    ];

    println!("请选择思维模式：");
    for (i, m) in modes.iter().enumerate() {
        println!("  {}) {} - {}", i + 1, m.label(), mode_description(*m));
    }

    let choice = match prompt_choice("输入编号", modes.len())? {
        Some(c) => c,
        None => return Ok(CmdOutcome::Continue),
    };

    let new_mode = modes[choice - 1];
    *ctx.thinking.lock().unwrap() = new_mode;
    colors::print_info(&format!("✓ 思考模式已切到 `{}`", new_mode.label()));
    println!();
    Ok(CmdOutcome::Continue)
}

fn mode_description(mode: ThinkingMode) -> &'static str {
    match mode {
        ThinkingMode::Direct => "直接回答，不展示思考过程",
        ThinkingMode::CoT => "先列出推理步骤再给出结论",
        ThinkingMode::ToT => "多分支探索后选最优方案",
        ThinkingMode::ReAct => "推理与工具调用交替进行",
        ThinkingMode::Plan => "复杂任务先出计划再执行",
    }
}

// ---------------- 语言向导 ----------------

async fn configure_lang(ctx: &AppContext) -> Result<CmdOutcome> {
    use crate::ui::colors;

    println!();
    println!("配置语言");
    println!("---------");

    let current = ctx.settings.lock().unwrap().lang.clone();
    println!("当前语言: {current}");
    println!("  1) 中文 (zh)");
    println!("  2) 英文 (en)");

    let new = match prompt_choice("输入编号", 2)? {
        Some(1) => "zh".to_string(),
        Some(2) => "en".to_string(),
        _ => return Ok(CmdOutcome::Continue),
    };

    {
        let mut settings = ctx.settings.lock().unwrap();
        settings.lang = new.clone();
    }
    crate::config::settings::save(&ctx.settings.lock().unwrap())?;

    // 重建 system prompt
    let sys_prompt = crate::llm::prompts::default_system_prompt(&new);
    {
        let mut session = ctx.session.lock().unwrap();
        if let Some(first) = session.messages.first_mut() {
            first.content = sys_prompt.clone();
        } else {
            session.messages.insert(0, crate::llm::message::Message::system(sys_prompt));
        }
    }

    colors::print_info(&format!("✓ 已切换语言为 `{new}`，system prompt 已重建"));
    println!();
    Ok(CmdOutcome::Continue)
}

// ---------------- 自治模式向导 ----------------

async fn configure_autonomous(ctx: &AppContext) -> Result<CmdOutcome> {
    use crate::ui::colors;

    println!();
    println!("配置自治模式");
    println!("-------------");

    let current = ctx.settings.lock().unwrap().autonomous;
    println!("当前自治模式: {}", if current { "on" } else { "off" });
    println!("  1) 开启 (on)  — 工具调用不再征求确认");
    println!("  2) 关闭 (off) — 危险工具调用前需要确认");

    let on = match prompt_choice("输入编号", 2)? {
        Some(1) => true,
        Some(2) => false,
        _ => return Ok(CmdOutcome::Continue),
    };

    {
        let mut settings = ctx.settings.lock().unwrap();
        settings.autonomous = on;
    }
    crate::config::settings::save(&ctx.settings.lock().unwrap())?;

    {
        let mut permission = ctx.permission.lock().unwrap();
        permission.set_full_auto(on);
    }

    colors::print_info(&format!("✓ autonomous = {}", if on { "on" } else { "off" }));
    println!();
    Ok(CmdOutcome::Continue)
}

// ---------------- 单轮限制向导 ----------------

async fn configure_limit(ctx: &AppContext) -> Result<CmdOutcome> {
    use crate::ui::colors;

    println!();
    println!("配置单轮 max_tokens");
    println!("-------------------");

    let current = ctx.settings.lock().unwrap().limit;
    println!("当前 limit: {current}");

    let n: u32 = loop {
        let line = prompt_line("请输入新的 limit [50-8192]: ")?;
        let line = line.trim();
        if line.is_empty() {
            return Ok(CmdOutcome::Continue);
        }
        match line.parse::<u32>() {
            Ok(v) if (50..=8192).contains(&v) => break v,
            _ => colors::print_error("请输入 50-8192 之间的整数"),
        }
    };

    {
        let mut settings = ctx.settings.lock().unwrap();
        settings.limit = n;
    }
    crate::config::settings::save(&ctx.settings.lock().unwrap())?;

    colors::print_info(&format!("✓ limit = {n}"));
    println!();
    Ok(CmdOutcome::Continue)
}

// ---------------- 通用输入辅助 ----------------

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf)
}

fn prompt_choice(prompt: &str, max: usize) -> Result<Option<usize>> {
    loop {
        let line = prompt_line(&format!("{} [1-{}, 0=退出]: ", prompt, max))?;
        let line = line.trim();
        if line.is_empty() || line == "0" || line.eq_ignore_ascii_case("q") {
            return Ok(None);
        }
        match line.parse::<usize>() {
            Ok(n) if n >= 1 && n <= max => return Ok(Some(n)),
            _ => crate::ui::colors::print_error("无效选择，请重新输入"),
        }
    }
}

fn prompt_yes_no(prompt: &str, default: bool) -> Result<bool> {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        let line = prompt_line(&format!("{} {}: ", prompt, hint))?;
        let s = line.trim().to_lowercase();
        if s.is_empty() {
            return Ok(default);
        }
        match s.as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => crate::ui::colors::print_error("请输入 y 或 n"),
        }
    }
}

/// 简单隐藏输入（终端 echo off）。非 TTY 或失败时退化为普通 prompt_line。
fn prompt_sensitive(prompt: &str) -> Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        let is_tty = unsafe { libc::isatty(fd) } == 1;
        if !is_tty {
            return prompt_line(prompt);
        }
        let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
        let ok = unsafe { libc::tcgetattr(fd, &mut termios) } == 0;
        if ok {
            let mut new = termios;
            new.c_lflag &= !libc::ECHO;
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &new) };
        }
        print!("{}", prompt);
        let _ = io::stdout().flush();
        let mut buf = String::new();
        let res = io::stdin().read_line(&mut buf);
        if ok {
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) };
        }
        println!();
        res?;
        return Ok(buf);
    }

    #[cfg(not(unix))]
    {
        prompt_line(prompt)
    }
}
