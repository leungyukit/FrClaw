//! macOS `sandbox-exec` 增强。
//!
//! 当策略里 `use_macos_sandbox_exec = true` 且 `target_os = "macos"` 时，
//! shell 调用会包一层 `sandbox-exec -f <scheme>`，让 macOS 内核做 syscall 级隔离。
//!
//! 其他平台返回 None（不增强）。
//!
//! Scheme 模板：
//! ```text
//! (version 1)
//! (deny default)
//! (allow process-exec)              ; 让 sh -c 跑得起来
//! (allow process-fork)              ; 子进程
//! (allow sysctl-read)               ; sh 启动需要
//! (allow file-read* (subpath "/"))
//! (allow file-write* (subpath "<write_allow>"))
//! (allow network*)
//! ```

use super::policy::SandboxPolicy;
use std::path::Path;

pub fn is_supported() -> bool {
    cfg!(target_os = "macos")
}

/// 把 strategy 编译成 sandbox-exec scheme 文件内容。
pub fn build_scheme(policy: &SandboxPolicy) -> String {
    let mut s = String::new();
    s.push_str("(version 1)\n");
    s.push_str("(deny default)\n");
    s.push_str("(allow process-exec)\n");
    s.push_str("(allow process-fork)\n");
    s.push_str("(allow sysctl-read)\n");
    s.push_str("(allow mach-lookup)\n");
    s.push_str("(allow file-read* (subpath \"/\"))\n");
    // write 限制
    for path in policy.expanded_write_allow() {
        let path = sanitize_path(&path);
        s.push_str(&format!("(allow file-write* (subpath \"{path}\"))\n"));
    }
    // network
    match policy.network {
        super::policy::NetworkPolicy::Allow => {
            s.push_str("(allow network*)\n");
        }
        super::policy::NetworkPolicy::Deny => {
            s.push_str("(deny network*)\n");
        }
    }
    s
}

/// scheme 写到临时文件，返回路径。
pub fn write_temp_scheme(policy: &SandboxPolicy) -> std::io::Result<std::path::PathBuf> {
    let scheme = build_scheme(policy);
    let dir = std::env::temp_dir().join("fr-cli-sandbox");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("sb-{}.sb", std::process::id()));
    std::fs::write(&path, scheme)?;
    Ok(path)
}

/// 把 cmd 用 sandbox-exec 包装。
/// 返回 (program, args) —— program 变成 sandbox-exec，args 变成 ["-f", scheme, "sh", "-c", cmd]。
pub fn wrap_command(policy: &SandboxPolicy, cmd: &str) -> Option<(String, Vec<String>)> {
    if !policy.use_macos_sandbox_exec || !is_supported() {
        return None;
    }
    let scheme_path = write_temp_scheme(policy).ok()?;
    Some((
        "sandbox-exec".to_string(),
        vec![
            "-f".to_string(),
            scheme_path.to_string_lossy().to_string(),
            "sh".to_string(),
            "-c".to_string(),
            cmd.to_string(),
        ],
    ))
}

fn sanitize_path(p: &str) -> String {
    // scheme 解析：subpath 路径里不能有引号
    p.replace('"', "")
}

#[allow(dead_code)]
pub fn is_subpath(child: &Path, parent: &Path) -> bool {
    child.starts_with(parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::policy::SandboxPolicy;

    #[test]
    fn scheme_contains_deny_default() {
        let p = SandboxPolicy::default();
        let s = build_scheme(&p);
        assert!(s.contains("(deny default)"));
        assert!(s.contains("(allow process-exec)"));
    }

    #[test]
    fn network_deny_appears_when_denied() {
        let mut p = SandboxPolicy::default();
        p.network = super::super::policy::NetworkPolicy::Deny;
        let s = build_scheme(&p);
        assert!(s.contains("(deny network*)"));
        assert!(!s.contains("(allow network*)"));
    }

    #[test]
    fn wrap_command_returns_none_on_linux() {
        // 这个测试在非 macOS 上能跑（cfg 守卫）
        let p = SandboxPolicy::default();
        // 不要在 macOS 上 wrap（scheme 写盘会变慢）—— 测试在非 macOS 跳过
        if !is_supported() {
            assert!(wrap_command(&p, "ls").is_none());
        }
    }
}
