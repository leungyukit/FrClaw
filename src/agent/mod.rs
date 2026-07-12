//! Agent 套件：MasterAgent 的 ReAct 循环（think → tool → observe）。

pub mod loop_runner;
pub mod prompts;

pub use loop_runner::{run_agent_step_loop, AgentRunOptions, AgentStep};
pub use prompts::ThinkingMode;
