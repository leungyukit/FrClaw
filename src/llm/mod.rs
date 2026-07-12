//! LLM 子模块。
//!
//! - [`message`] —— 跨协议统一的消息 / 角色 / tool call 模型
//! - [`provider`] —— Provider 抽象 trait
//! - [`openai_compat`] —— OpenAI 兼容协议实现，覆盖大部分国产/海外厂商
//! - [`registry`] —— Provider 工厂 + 降级链
//! - [`prompts`] —— system prompt 模板（中英文）

pub mod message;
pub mod openai_compat;
pub mod provider;
pub mod prompts;
pub mod registry;

pub use message::{Message, Role, ToolCall, ToolDefinition, ToolResult};
pub use provider::{CompletionRequest, CompletionResponse, LlmProvider, StreamEvent};
pub use registry::{build_provider, FallbackChain};
