//! 沙箱隔离。
//!
//! 设计目标：给 `shell` / `read_file` / `write_file` / `list_dir` 工具加一层「绝不踩雷」护栏。
//!
//! 三层防御：
//! 1. **路径白名单**：读/写必须落在允许的目录（cwd、~/.fr_cli/、/tmp/ 默认开）
//! 2. **shell 命令黑名单**：`rm -rf /`、fork bomb、`mkfs`、`dd if=` 等高危命令
//! 3. **资源限制**：stdout 上限、超时
//!
//! macOS 增强：用 `sandbox-exec -f scheme.sb` 包裹 shell 调用（用户开关）。
//!
//! 用户可关（`/sandbox off`）—— 但默认开。

pub mod check;
pub mod macos;
pub mod policy;

pub use check::{check_path, check_shell, PathOp, SandboxVerdict};
pub use policy::{NetworkPolicy, SandboxPolicy};
