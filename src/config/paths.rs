//! `~/.fr_cli/` 路径常量与目录创建。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 用户主目录下的 fr_cli 数据目录。
pub fn data_dir() -> Result<PathBuf> {
    let base = dirs::home_dir()
        .or_else(dirs::data_dir)
        .context("无法确定用户主目录")?;
    let dir = base.join(".fr_cli");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// 会话文件存放目录（`~/.fr_cli/sessions/`）。
pub fn sessions_dir() -> Result<PathBuf> {
    let dir = data_dir()?.join("sessions");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// 配置文件目录（`~/.fr_cli/` 内）。
pub fn models_yaml_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("models.yaml"))
}

pub fn keys_json_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("keys.json"))
}

pub fn settings_json_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("settings.json"))
}

/// Round 7 ─ RAG 数据库路径（`~/.fr_cli/rag.db`）。
pub fn rag_db_path() -> PathBuf {
    data_dir().map(|d| d.join("rag.db")).unwrap_or_else(|_| PathBuf::from("./rag.db"))
}

/// Round 9 ─ Sandbox 策略文件路径（`~/.fr_cli/sandbox.json`）。
pub fn sandbox_json_path() -> PathBuf {
    data_dir().map(|d| d.join("sandbox.json")).unwrap_or_else(|_| PathBuf::from("./sandbox.json"))
}

/// Round 12 ─ Hermes 任务数据库路径（`~/.fr_cli/tasks.db`）。
pub fn hermes_db_path() -> PathBuf {
    data_dir().map(|d| d.join("tasks.db")).unwrap_or_else(|_| PathBuf::from("./tasks.db"))
}

fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)
            .with_context(|| format!("创建目录失败: {}", path.display()))?;
    }
    Ok(())
}
