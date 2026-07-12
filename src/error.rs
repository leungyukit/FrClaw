//! 统一错误类型。
//!
//! 所有模块共享 `Result<T>` 别名与 `Error` enum。
//! LLM 调用、IO、配置解析等失败统一收敛为 `Error`，
//! 通过 `anyhow::Error` 兼容其他错误源。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("llm provider `{provider}` error: {message}")]
    Llm { provider: String, message: String },

    #[error("auth error: missing or invalid API key for provider `{0}`")]
    MissingApiKey(String),

    #[error("session error: {0}")]
    Session(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Error::Other(format!("{value:#}"))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
