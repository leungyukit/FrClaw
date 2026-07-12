//! 沙箱策略（policy + 默认值 + 持久化）。
//!
//! 持久化到 `~/.fr_cli/sandbox.json`。
//!
//! ```json
//! {
//!   "enabled": true,
//!   "read_allow": ["$cwd", "$HOME/.fr_cli", "/tmp"],
//!   "write_allow": ["$cwd", "$HOME/.fr_cli"],
//!   "shell_deny": ["rm -rf /", "mkfs", ...],
//!   "network": "allow",
//!   "max_stdout_bytes": 1048576,
//!   "timeout_ms": 30000,
//!   "use_macos_sandbox_exec": true
//! }
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPolicy {
    Allow,
    Deny,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        NetworkPolicy::Allow
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 读路径白名单（substring 匹配，绝对路径前缀）
    #[serde(default)]
    pub read_allow: Vec<String>,
    /// 写路径白名单
    #[serde(default)]
    pub write_allow: Vec<String>,
    /// shell 命令黑名单（substring 匹配）
    #[serde(default)]
    pub shell_deny: Vec<String>,
    /// shell 命令白名单（substring 匹配；空 = 全部允许，除了 deny）
    #[serde(default)]
    pub shell_allow: Vec<String>,
    /// 网络策略
    #[serde(default)]
    pub network: NetworkPolicy,
    /// 单次命令 stdout 上限
    #[serde(default = "default_max_stdout")]
    pub max_stdout_bytes: usize,
    /// 单次命令超时（毫秒）
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    /// macOS 平台：用 sandbox-exec 包裹 shell 调用
    #[serde(default = "default_true")]
    pub use_macos_sandbox_exec: bool,
}

fn default_true() -> bool {
    true
}

fn default_max_stdout() -> usize {
    1024 * 1024 // 1 MB
}

fn default_timeout() -> u64 {
    30_000 // 30s
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        let (read_allow, write_allow) = default_path_lists();
        let shell_deny = default_shell_deny();
        Self {
            enabled: true,
            read_allow,
            write_allow,
            shell_deny,
            shell_allow: vec![],
            network: NetworkPolicy::Allow,
            max_stdout_bytes: 1024 * 1024,
            timeout_ms: 30_000,
            use_macos_sandbox_exec: cfg!(target_os = "macos"),
        }
    }
}

/// 默认路径白名单：cwd + ~/.fr_cli + /tmp。
fn default_path_lists() -> (Vec<String>, Vec<String>) {
    let mut read = vec![
        "${cwd}".to_string(),
        "${home}/.fr_cli".to_string(),
        "/tmp".to_string(),
        "/private/tmp".to_string(),
        "/private/var/folders".to_string(),
        "/Users".to_string(),  // 沙箱默认开 macOS 允许读 user home
    ];
    let cwd = std::env::current_dir().ok();
    let home = dirs::home_dir();
    let mut write = vec![
        "${cwd}".to_string(),
        "${home}/.fr_cli".to_string(),
        "/tmp".to_string(),
    ];
    if let Some(c) = cwd {
        read.push(c.to_string_lossy().to_string());
        write.push(c.to_string_lossy().to_string());
    }
    if let Some(h) = home {
        read.push(h.to_string_lossy().to_string());
    }
    (read, write)
}

/// 默认 shell 黑名单。
fn default_shell_deny() -> Vec<String> {
    vec![
        // 致命删
        "rm -rf /".into(),
        "rm -rf /*".into(),
        "rm -rf $HOME".into(),
        "rm -rf ~".into(),
        // fork bomb（多种空格变体）
        ":(){:|:&};:".into(),
        ":(){ :|:& };:".into(),
        ":(){: |:& };:".into(),
        // 磁盘
        "mkfs".into(),
        "dd if=".into(),
        "fdisk".into(),
        // 系统
        "shutdown".into(),
        "reboot".into(),
        "halt".into(),
        "poweroff".into(),
        "init 0".into(),
        "init 6".into(),
        // 写到原始设备
        "> /dev/sd".into(),
        "> /dev/nvme".into(),
        "> /dev/disk".into(),
        // 危险 su / chmod
        "chmod -R 777 /".into(),
        "chown -R".into(),
        // 远程下载 + 执行（任意 URL 都拦）
        " | sh".into(),
        " | bash".into(),
        " | zsh".into(),
        " |sudo sh".into(),
        "$(curl".into(),
        "$(wget".into(),
    ]
}

/// 把 `${cwd}` / `${home}` 展开成实际路径。
pub fn expand_token(s: &str, cwd: &Path, home: &Path) -> String {
    s.replace("${cwd}", &cwd.to_string_lossy())
        .replace("${home}", &home.to_string_lossy())
        .replace("~", &home.to_string_lossy())
}

/// 策略文件路径 `~/.fr_cli/sandbox.json`。
pub fn sandbox_json_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".fr_cli").join("sandbox.json"))
        .unwrap_or_else(|| PathBuf::from("./sandbox.json"))
}

impl SandboxPolicy {
    /// 从 `~/.fr_cli/sandbox.json` 加载；不存在则返回默认。
    pub fn load_or_default() -> Self {
        let path = sandbox_json_path();
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<SandboxPolicy>(&content) {
                Ok(p) => p,
                Err(_) => Self::default(),
            },
            Err(_) => Self::default(),
        }
    }

    /// 保存到 `~/.fr_cli/sandbox.json`。
    pub fn save(&self) -> Result<()> {
        let path = sandbox_json_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)
            .with_context(|| format!("写 sandbox 策略失败: {}", path.display()))?;
        Ok(())
    }

    /// 把 read_allow / write_allow 里的 `${cwd}` / `${home}` 展开成实际路径列表。
    pub fn expanded_read_allow(&self) -> Vec<String> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        self.read_allow
            .iter()
            .map(|s| expand_token(s, &cwd, &home))
            .collect()
    }
    pub fn expanded_write_allow(&self) -> Vec<String> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        self.write_allow
            .iter()
            .map(|s| expand_token(s, &cwd, &home))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_enabled() {
        let p = SandboxPolicy::default();
        assert!(p.enabled);
        assert!(!p.read_allow.is_empty());
        assert!(!p.write_allow.is_empty());
        assert!(!p.shell_deny.is_empty());
    }

    #[test]
    fn default_deny_includes_fork_bomb_and_rm_rf() {
        let p = SandboxPolicy::default();
        assert!(p.shell_deny.iter().any(|s| s.contains("rm -rf /")));
        assert!(p.shell_deny.iter().any(|s| s.contains(":()")));
        assert!(p.shell_deny.iter().any(|s| s.contains("mkfs")));
        assert!(p.shell_deny.iter().any(|s| s.contains("dd if=")));
        assert!(p.shell_deny.iter().any(|s| s.contains("shutdown")));
    }

    #[test]
    fn default_deny_includes_pipe_to_sh() {
        let p = SandboxPolicy::default();
        assert!(p.shell_deny.iter().any(|s| s.contains(" | sh")));
    }

    #[test]
    fn expand_token_replaces_cwd_and_home() {
        let cwd = Path::new("/tmp/work");
        let home = Path::new("/Users/me");
        assert_eq!(expand_token("${cwd}/x", cwd, home), "/tmp/work/x");
        assert_eq!(expand_token("${home}/.fr_cli", cwd, home), "/Users/me/.fr_cli");
        assert_eq!(expand_token("~/x", cwd, home), "/Users/me/x");
    }

    #[test]
    fn expanded_allow_resolves_tokens() {
        let mut p = SandboxPolicy::default();
        p.read_allow = vec!["${cwd}/data".into(), "${home}/notes".into()];
        let r = p.expanded_read_allow();
        assert!(r.iter().any(|s| s.ends_with("/data")));
        assert!(r.iter().any(|s| s.ends_with("/notes")));
    }
}
