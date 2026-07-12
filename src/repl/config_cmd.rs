//! 交互式配置命令 (`/config`)。
//!
//! 当前支持：
//!   - 配置模型 provider：选择预设 → 选择模型 → 确认 base_url → 输入 api key → 设为 default。

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
        return Ok(CmdOutcome::Continue);
    }

    match args[0] {
        "model" | "m" => configure_model(ctx).await,
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
    println!("  /config model   配置或添加 LLM provider");
    println!();
}

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

    let choice = read_line("输入编号: ")?;
    let choice: usize = match choice.trim().parse() {
        Ok(n) if n >= 1 && n <= PRESETS.len() + 1 => n,
        _ => {
            crate::ui::colors::print_error("无效选择");
            return Ok(CmdOutcome::Continue);
        }
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

        let model_choice = read_line("输入编号: ")?;
        let model_choice: usize = match model_choice.trim().parse() {
            Ok(n) if n >= 1 && n <= preset.models.len() + 1 => n,
            _ => {
                crate::ui::colors::print_error("无效选择");
                return Ok(CmdOutcome::Continue);
            }
        };

        let model = if model_choice <= preset.models.len() {
            preset.models[model_choice - 1].to_string()
        } else {
            read_line("输入模型名: ")?.trim().to_string()
        };

        // 3. 确认 base_url
        println!();
        let input = read_line(&format!("base_url [{}]: ", preset.base_url))?;
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
        let alias = read_line("provider alias (唯一标识，如 my-openai): ")?;
        let alias = alias.trim().to_string();
        if alias.is_empty() {
            crate::ui::colors::print_error("alias 不能为空");
            return Ok(CmdOutcome::Continue);
        }

        let name = read_line("显示名称: ")?;
        let name = name.trim().to_string();
        let name = if name.is_empty() { alias.clone() } else { name };

        let base_url = read_line("base_url: ")?;
        let base_url = base_url.trim().to_string();
        if base_url.is_empty() {
            crate::ui::colors::print_error("base_url 不能为空");
            return Ok(CmdOutcome::Continue);
        }

        let model = read_line("模型名: ")?;
        let model = model.trim().to_string();
        if model.is_empty() {
            crate::ui::colors::print_error("模型名 不能为空");
            return Ok(CmdOutcome::Continue);
        }

        let api_key_env = read_line("API key 环境变量名 (可直接回车跳过): ")?;
        let api_key_env = {
            let s = api_key_env.trim();
            if s.is_empty() { None } else { Some(s.to_string()) }
        };

        (alias, name, base_url, api_key_env, model)
    };

    // 4. 输入 api key
    println!();
    let api_key = read_line_sensitive("API key (输入不会显示): ")?;
    let api_key = api_key.trim().to_string();

    // 5. 是否设为 default
    let set_default = {
        let input = read_line("是否设为默认 provider? [Y/n]: ")?;
        let s = input.trim().to_lowercase();
        s.is_empty() || s == "y" || s == "yes"
    };

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

fn read_line(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf)
}

/// 简单隐藏输入（终端 echo off）。非 TTY 或失败时退化为普通 read_line。
fn read_line_sensitive(prompt: &str) -> Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        let is_tty = unsafe { libc::isatty(fd) } == 1;
        if !is_tty {
            return read_line(prompt);
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
        read_line(prompt)
    }
}
