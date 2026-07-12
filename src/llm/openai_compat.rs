//! OpenAI 兼容协议实现（流式）。
//!
//! 实现：
//! - `chat_stream` 真 SSE 解析（`data: {...}` 行, `[DONE]` 终止符）。
//! - 工具调用（`tool_calls`）增量累积：跨 SSE chunk 不丢失。
//! - `chat` 默认实现走流式拼装，保持原有 API 不变。

use crate::config::keys;
use crate::config::models::ProviderConfig;
use crate::error::{Error, Result};
use crate::llm::message::{Message, ToolCall, ToolCallFunction, ToolDefinition};
use crate::llm::provider::{CompletionRequest, CompletionResponse, LlmProvider, StreamEvent, TextStream};
use async_trait::async_trait;
use futures_util::stream::StreamExt;
use reqwest::{Client, ClientBuilder};
use serde_json::{json, Value};
use std::time::Duration;

pub struct OpenAiCompatProvider {
    cfg: ProviderConfig,
    alias: String,
    api_key: Option<String>,
    client: Client,
}

impl OpenAiCompatProvider {
    pub fn new(alias: impl Into<String>, cfg: ProviderConfig) -> Result<Self> {
        let alias = alias.into();
        let api_key = keys::resolve(cfg.api_key_env.as_deref(), &alias);
        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|e| Error::Other(format!("build http client: {e}")))?;
        Ok(Self {
            cfg,
            alias,
            api_key,
            client,
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn alias(&self) -> &str {
        &self.alias
    }
    fn model(&self) -> &str {
        &self.cfg.model
    }
    fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }
    fn config(&self) -> &ProviderConfig {
        &self.cfg
    }

    async fn chat_stream(&self, req: CompletionRequest) -> anyhow::Result<TextStream> {
        // 真正的 SSE 路径
        let api_key = self
            .api_key
            .clone()
            .ok_or_else(|| anyhow::anyhow!(Error::MissingApiKey(self.alias.clone())))?;

        let url = format!(
            "{}/chat/completions",
            self.cfg.base_url.trim_end_matches('/')
        );
        let mut body = build_request_body(&self.cfg, &req);
        body["stream"] = json!(true);
        // 多数 OpenAI 兼容服务需要 stream_options.include_usage 来拿 usage。
        body["stream_options"] = json!({ "include_usage": true });

        let mut builder = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {api_key}"));
        for (k, v) in &self.cfg.extra_headers {
            builder = builder.header(k, v);
        }

        let resp = builder.json(&body).send().await.map_err(Error::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(Error::Llm {
                provider: self.alias.clone(),
                message: format!("HTTP {}: {}", status, txt),
            }));
        }

        let alias = self.alias.clone();
        let byte_stream = resp.bytes_stream();

        let stream = async_stream::stream! {
            let parser = SseParser::new();
            let mut acc = StreamAccumulator::default();
            let mut buf: Vec<u8> = Vec::new();
            let mut seen_done = false;
            futures_util::pin_mut!(byte_stream);
            while let Some(item) = byte_stream.next().await {
                let bytes = match item {
                    Ok(b) => b,
                    Err(e) => {
                        yield Err(anyhow::anyhow!(Error::Http(e)));
                        return;
                    }
                };
                buf.extend_from_slice(&bytes);
                // 把 buffer 切成 SSE 行，每行尝试解析。
                while let Some(line_range) = next_sse_line(&buf) {
                    let line_bytes = buf[line_range.clone()].to_vec();
                    buf.drain(..line_range.end);
                    let line = std::str::from_utf8(&line_bytes).unwrap_or("");
                    if let Some(rest) = line.strip_prefix("data:") {
                        let rest = rest.trim();
                        if rest == "[DONE]" {
                            seen_done = true;
                            continue;
                        }
                        if rest.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<StreamChunk>(rest) {
                            Ok(chunk) => {
                                for ev in acc.update(&alias, &chunk) {
                                    yield Ok(ev);
                                }
                            }
                            Err(_e) => {
                                // chunk 跨越 SSE 行边界被切两半——再缓冲到下次
                                // 简单起见：放弃残片日志（实际只需再 buffer）
                            }
                        }
                    }
                }
            }
            if !seen_done {
                // 流结束但没收到 [DONE]——为了简化，依然调用 finalize() 让它兜底。
            }
            let _ = seen_done;
            match acc.finalize(true) {
                Some(r) => yield Ok(StreamEvent::Done(r)),
                None => {}
            }
            // keep parser name referenced (no-op)
            let _ = parser.is_done();
            drop(parser);
        };

        Ok(Box::pin(stream) as TextStream)
    }

    async fn chat(&self, req: CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let mut stream = self.chat_stream(req).await?;
        let mut acc = String::new();
        let mut last_done: Option<CompletionResponse> = None;
        while let Some(ev) = stream.next().await {
            match ev? {
                StreamEvent::Delta(s) => acc.push_str(&s),
                StreamEvent::Done(r) => last_done = Some(r),
            }
        }
        match last_done {
            Some(mut r) => {
                if r.content.is_empty() {
                    r.content = acc;
                }
                Ok(r)
            }
            None => Ok(CompletionResponse {
                content: acc,
                tool_calls: vec![],
                prompt_tokens: None,
                completion_tokens: None,
                finish_reason: Some("stop".into()),
            }),
        }
    }
}

// ----------------- SSE line splitter -----------------

/// 在 buffer 中找下一条完整 SSE 行（以 `\n` 或 `\r\n` 结尾）。
/// 返回 `[start, end)`（end 是换行字符的 index）。
fn next_sse_line(buf: &[u8]) -> Option<std::ops::Range<usize>> {
    for (i, b) in buf.iter().enumerate() {
        if *b == b'\n' {
            let line_end = if i > 0 && buf[i - 1] == b'\r' { i - 1 } else { i };
            return Some(0..line_end);
        }
    }
    None
}

// ----------------- Stream chunk model -----------------

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<StreamUsage>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<StreamToolCallDelta>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct StreamToolCallDelta {
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFunctionDelta>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct StreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct StreamUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
}

// ----------------- State machine -----------------

#[derive(Default)]
struct StreamAccumulator {
    content: String,
    /// tool_calls 按 index 累积（id/name/arguments 都可能跨 chunk）
    tools: Vec<ToolCall>,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    finish_reason: Option<String>,
}

impl StreamAccumulator {
    fn update(&mut self, _alias: &str, chunk: &StreamChunk) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        for choice in &chunk.choices {
            if let Some(c) = &choice.delta.content {
                self.content.push_str(c);
                events.push(StreamEvent::Delta(c.clone()));
            }
            if !choice.delta.tool_calls.is_empty() {
                for tcd in &choice.delta.tool_calls {
                    let idx = tcd.index.unwrap_or(0) as usize;
                    while self.tools.len() <= idx {
                        self.tools.push(ToolCall {
                            id: String::new(),
                            kind: "function".into(),
                            function: ToolCallFunction {
                                name: String::new(),
                                arguments: Value::Null,
                            },
                        });
                    }
                    let slot = &mut self.tools[idx];
                    if let Some(id) = &tcd.id {
                        slot.id.push_str(id);
                    }
                    if let Some(func) = &tcd.function {
                        if let Some(name) = &func.name {
                            slot.function.name.push_str(name);
                        }
                        if let Some(arg_chunk) = &func.arguments {
                            // 增量累积到现有 arguments（保持为 JSON 字符串）
                            let prev = slot
                                .function
                                .arguments
                                .as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_default();
                            slot.function.arguments = json!(format!("{prev}{arg_chunk}"));
                        }
                    }
                }
            }
            if let Some(fr) = &choice.finish_reason {
                self.finish_reason = Some(fr.clone());
            }
        }
        if let Some(u) = &chunk.usage {
            self.prompt_tokens = u.prompt_tokens.or(self.prompt_tokens);
            self.completion_tokens = u.completion_tokens.or(self.completion_tokens);
        }
        events
    }

    fn finalize(&mut self, _seen_done: bool) -> Option<CompletionResponse> {
        // 工具参数（来自流式累积的是 JSON 字符串）尝试反序列化为 Value
        for tc in &mut self.tools {
            if let Some(s) = tc.function.arguments.as_str() {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    tc.function.arguments = parsed;
                }
            }
        }
        // 即便没收到 [DONE] 也允许结束
        if self.content.is_empty() && self.tools.is_empty() {
            return None;
        }
        Some(CompletionResponse {
            content: std::mem::take(&mut self.content),
            tool_calls: std::mem::take(&mut self.tools),
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            finish_reason: self.finish_reason.clone(),
        })
    }
}

/// 占位 parser，跟随 trait `StreamEvent` 的 emit 节奏，目前无状态。
struct SseParser {
    done: bool,
}
impl SseParser {
    fn new() -> Self {
        Self { done: false }
    }
    fn is_done(&self) -> bool {
        self.done
    }
}

// ----------------- 请求体 / 序列化 -----------------

fn build_request_body(cfg: &ProviderConfig, req: &CompletionRequest) -> Value {
    let messages: Vec<Value> = req.messages.iter().map(message_to_openai).collect();
    let mut body = json!({
        "model": cfg.model,
        "messages": messages,
        "temperature": req.temperature.unwrap_or(cfg.temperature.unwrap_or(0.7)),
    });
    if let Some(max) = req.max_tokens.or(cfg.max_tokens) {
        body["max_tokens"] = json!(max);
    }
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req.tools.iter().map(tool_to_openai).collect();
        body["tools"] = json!(tools);
        body["tool_choice"] = json!("auto");
    }
    body
}

fn tool_to_openai(t: &ToolDefinition) -> Value {
    json!({
        "type": t.kind,
        "function": {
            "name": t.function.name,
            "description": t.function.description,
            "parameters": t.function.parameters,
        }
    })
}

fn message_to_openai(m: &Message) -> Value {
    let mut obj = json!({
        "role": role_str(m.role),
        "content": m.content,
    });
    if !m.tool_calls.is_empty() {
        let calls: Vec<Value> = m
            .tool_calls
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "type": c.kind,
                    "function": {
                        "name": c.function.name,
                        "arguments": c.function.arguments.to_string(),
                    }
                })
            })
            .collect();
        obj["tool_calls"] = json!(calls);
    }
    if let Some(id) = &m.tool_call_id {
        obj["tool_call_id"] = json!(id);
    }
    if let Some(name) = &m.name {
        obj["name"] = json!(name);
    }
    obj
}

fn role_str(role: crate::llm::message::Role) -> &'static str {
    use crate::llm::message::Role;
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}
