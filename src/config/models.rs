//! `models.yaml` 解析。
//!
//! 文件 schema 与原 fr-cli 的 `models.yaml` 兼容——
//! 字段名略做简化（payload 不再依赖 `client` 枚举值）。
//! 项目默认不内置任何 provider，首次启动会生成空模板，用户需自行配置：
//!
//! ```yaml
//! providers:
//!   deepseek:
//!     name: "DeepSeek"
//!     model: "deepseek-chat"
//!     protocol: openai    # 协议：openai / anthropic（仅 openai 落地）
//!     base_url: "https://api.deepseek.com/v1"
//!     api_key_env: "DEEPSEEK_API_KEY"
//!     max_tokens: 8192
//!     temperature: 0.7
//!     is_default: true
//!     is_backup: false
//! settings:
//!   default_provider: "deepseek"
//!   backup_provider: ~
//!   history_window: 5
//! ```

use crate::config::paths;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsFile {
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub settings: GlobalSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub model: String,
    /// 协议类型：`openai` / `anthropic`（当前只实现 `openai`）
    pub protocol: String,
    pub base_url: String,
    /// 用于获取 API key 的环境变量名（如 `OPENAI_API_KEY`）
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub is_backup: bool,
    #[serde(default)]
    pub extra_headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalSettings {
    pub default_provider: Option<String>,
    pub backup_provider: Option<String>,
    /// 注入 system prompt 的近期对话轮数。
    #[serde(default = "default_history_window")]
    pub history_window: usize,
    /// 语言：`zh` / `en`
    #[serde(default = "default_lang")]
    pub lang: String,
    /// 每次请求的最大 token 上限（粗略保护）。
    #[serde(default = "default_max_tokens_limit")]
    pub max_tokens_limit: u32,
}

fn default_history_window() -> usize {
    5
}

fn default_lang() -> String {
    "zh".into()
}

fn default_max_tokens_limit() -> u32 {
    8192
}

/// 内置的兜底配置——首次启动 / `~/.fr_cli/models.yaml` 缺失时使用。
/// 默认不携带任何 provider，强制用户显式配置。
pub fn builtin_default() -> ModelsFile {
    ModelsFile {
        providers: BTreeMap::new(),
        settings: GlobalSettings {
            default_provider: None,
            backup_provider: None,
            history_window: 5,
            lang: "zh".into(),
            max_tokens_limit: 8192,
        },
    }
}

/// 加载配置：优先用户 `~/.fr_cli/models.yaml`，缺失则写一份内置默认并返回。
pub fn load_or_init() -> Result<ModelsFile> {
    let path = paths::models_yaml_path()?;
    if !path.exists() {
        let defaults = builtin_default();
        write_to(&path, &defaults)?;
        return Ok(defaults);
    }
    load_from(&path)
}

pub fn load_from(path: &Path) -> Result<ModelsFile> {
    let text = std::fs::read_to_string(path)?;
    let parsed: ModelsFile = serde_yaml::from_str(&text)?;
    Ok(parsed)
}

pub fn write_to(path: &Path, cfg: &ModelsFile) -> Result<()> {
    let text = serde_yaml::to_string(cfg)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

impl ModelsFile {
    /// 默认 provider 名（`settings.default_provider` 优先，否则取 `is_default=true`）。
    pub fn default_provider_name(&self) -> Option<String> {
        if let Some(name) = &self.settings.default_provider {
            return Some(name.clone());
        }
        self.providers
            .iter()
            .find(|(_, p)| p.is_default)
            .map(|(k, _)| k.clone())
    }

    /// 备用 provider 名。
    pub fn backup_provider_name(&self) -> Option<String> {
        if let Some(name) = &self.settings.backup_provider {
            return Some(name.clone());
        }
        self.providers
            .iter()
            .find(|(_, p)| p.is_backup)
            .map(|(k, _)| k.clone())
    }

    pub fn get(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut ProviderConfig> {
        self.providers.get_mut(name)
    }
}
