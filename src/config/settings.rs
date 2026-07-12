//! 运行期 settings：自治模式、limit、lang。
//!
//! 持久化在 `~/.fr_cli/settings.json`，进程里也能临时覆盖。

use crate::config::paths;
use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_autonomous")]
    pub autonomous: bool,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default = "default_lang")]
    pub lang: String,
}

fn default_autonomous() -> bool {
    false
}

fn default_limit() -> u32 {
    50
}

fn default_lang() -> String {
    "zh".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            autonomous: default_autonomous(),
            limit: default_limit(),
            lang: default_lang(),
        }
    }
}

pub fn load_or_default() -> Result<Settings> {
    let path = paths::settings_json_path()?;
    if !path.exists() {
        return Ok(Settings::default());
    }
    let text = std::fs::read_to_string(&path)?;
    if text.trim().is_empty() {
        return Ok(Settings::default());
    }
    let s: Settings = serde_json::from_str(&text).unwrap_or_default();
    Ok(s)
}

pub fn save(s: &Settings) -> Result<()> {
    let path = paths::settings_json_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(s).map_err(|e| {
        crate::Error::Other(format!("serialize settings: {e}"))
    })?;
    std::fs::write(&path, text)?;
    Ok(())
}
