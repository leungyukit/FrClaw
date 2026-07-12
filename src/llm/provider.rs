//! Provider 抽象 trait。

use crate::config::models::ProviderConfig;
use crate::llm::message::{Message, ToolDefinition};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::stream::Stream;
use std::pin::Pin;

pub type TextStream =
    Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;

/// 补全请求（= messages + 工具 + 可选采样参数）。
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// 强制非流式（用于测试 / 小输入）。
    pub force_non_stream: bool,
}

impl Default for CompletionRequest {
    fn default() -> Self {
        Self {
            messages: vec![],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            force_non_stream: false,
        }
    }
}

/// 补全响应。
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub content: String,
    pub tool_calls: Vec<crate::llm::message::ToolCall>,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub finish_reason: Option<String>,
}

/// 流式事件（增量文本）。
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Delta(String),
    Done(CompletionResponse),
}

/// Provider trait —— 不依赖具体协议。
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// provider 的代号（与 `models.yaml` 中的 key 一致）。
    fn alias(&self) -> &str;
    /// 当前使用的模型名。
    fn model(&self) -> &str;

    /// 是否携带 API key（false 表示 local / ollama）。
    fn has_api_key(&self) -> bool;

    /// 内部用的配置快照。
    fn config(&self) -> &ProviderConfig;

    /// 流式聊天补全。返回 `TextStream`，外层用 `.next().await` 拉数据。
    async fn chat_stream(&self, req: CompletionRequest) -> Result<TextStream>;

    /// 非流式补全（一次性）。
    async fn chat(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        // 默认实现：跑流式拼起来。
        use futures_util::StreamExt;
        let mut stream = self.chat_stream(req).await?;
        let mut acc = String::new();
        let mut last = None;
        while let Some(ev) = stream.next().await {
            match ev? {
                StreamEvent::Delta(s) => acc.push_str(&s),
                StreamEvent::Done(r) => last = Some(r),
            }
        }
        let mut last = last.unwrap_or_else(|| CompletionResponse {
            content: acc.clone(),
            tool_calls: vec![],
            prompt_tokens: None,
            completion_tokens: None,
            finish_reason: Some("stop".into()),
        });
        if last.content.is_empty() {
            last.content = acc;
        }
        Ok(last)
    }
}
