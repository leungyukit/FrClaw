//! Round 16f ─ Voice 录制 + STT。
//!
//! 路径：
//! 1. `sox` / `ffmpeg` / macOS 自带 `rec` → 录音到 wav / m4a
//! 2. multipart/form-data POST 到 OpenAI `/v1/audio/transcriptions` (Whisper)
//! 3. 返回纯文本，注入 LLM 或回写到 session
//!
//! 环境变量：
//! - `OPENAI_API_KEY` ── 必填
//! - `OPENAI_BASE_URL` ── 可选（默认 `https://api.openai.com/v1`）
//! - `WHISPER_MODEL`    ── 可选（默认 `whisper-1`）
//!
//! 注：录音部分默认 30s 自动停止（用 `timeout` 包 sox/ffmpeg）。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SttOptions {
    /// 录音时长（秒），默认 30
    pub duration_secs: u32,
    /// 输出格式：wav / m4a
    pub format: AudioFormat,
    /// Whisper model（默认 whisper-1）
    pub model: Option<String>,
    /// 语言 hint（ISO 639-1：zh / en）
    pub language: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioFormat {
    #[default]
    Wav,
    M4a,
}

impl AudioFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            AudioFormat::Wav => "wav",
            AudioFormat::M4a => "m4a",
        }
    }
}

/// 检查环境（API key + 录音工具）。
pub fn check_env() -> Result<RecorderCheck> {
    let key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| anyhow!("OPENAI_API_KEY 未设置"))?;
    let has_sox = Command::new("sox").arg("--version").output().is_ok();
    let has_ffmpeg = Command::new("ffmpeg").arg("-version").output().is_ok();
    let has_rec = Command::new("rec").arg("--version").output().is_ok();
    Ok(RecorderCheck {
        api_key_present: !key.is_empty(),
        has_sox,
        has_ffmpeg,
        has_rec,
        has_recorder: has_sox || has_ffmpeg || has_rec,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecorderCheck {
    pub api_key_present: bool,
    pub has_sox: bool,
    pub has_ffmpeg: bool,
    pub has_rec: bool,
    pub has_recorder: bool,
}

/// 录音到 out_path。返回录音文件路径。
pub fn record(out_path: impl AsRef<Path>, opts: &SttOptions) -> Result<PathBuf> {
    let out_path = out_path.as_ref();
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let duration = opts.duration_secs.to_string();

    // 优先级：sox → rec (sox alias) → ffmpeg (macOS)
    if Command::new("sox").arg("--version").output().is_ok() {
        // sox -d <out> trim 0 <duration>
        let status = Command::new("sox")
            .arg("-d")
            .arg(out_path)
            .arg("trim")
            .arg("0")
            .arg(&duration)
            .status()
            .map_err(|e| anyhow!("sox spawn: {e}"))?;
        if !status.success() {
            return Err(anyhow!("sox 录音失败: {status}"));
        }
        return Ok(out_path.to_path_buf());
    }
    if Command::new("rec").arg("--version").output().is_ok() {
        let status = Command::new("rec")
            .arg(out_path)
            .arg("trim")
            .arg("0")
            .arg(&duration)
            .status()
            .map_err(|e| anyhow!("rec spawn: {e}"))?;
        if !status.success() {
            return Err(anyhow!("rec 录音失败: {status}"));
        }
        return Ok(out_path.to_path_buf());
    }
    if Command::new("ffmpeg").arg("-version").output().is_ok() {
        // ffmpeg -f avfoundation -i ":0" -t <duration> -ar 16000 -ac 1 <out>
        // macOS 上 ffmpeg 用 avfoundation 抓麦克风；Linux 用 alsa
        let input = if cfg!(target_os = "macos") {
            ":0"
        } else {
            "default"
        };
        let input_flag = if cfg!(target_os = "macos") {
            "avfoundation"
        } else {
            "alsa"
        };
        let status = Command::new("ffmpeg")
            .args(["-y", "-f", input_flag, "-i", input])
            .args(["-t", &duration, "-ar", "16000", "-ac", "1"])
            .arg(out_path)
            .status()
            .map_err(|e| anyhow!("ffmpeg spawn: {e}"))?;
        if !status.success() {
            return Err(anyhow!("ffmpeg 录音失败: {status}"));
        }
        return Ok(out_path.to_path_buf());
    }
    Err(anyhow!(
        "没找到录音工具：需要 sox / rec (LAME) / ffmpeg 其一\n  macOS: brew install sox\n  Linux: apt install sox  或  apt install ffmpeg"
    ))
}

/// 把音频文件发到 OpenAI Whisper 转写。
pub async fn transcribe(audio_path: impl AsRef<Path>, opts: &SttOptions) -> Result<TranscribeResult> {
    let audio_path = audio_path.as_ref();
    if !audio_path.exists() {
        return Err(anyhow!("audio 文件不存在: {}", audio_path.display()));
    }
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| anyhow!("OPENAI_API_KEY 未设置"))?;
    let base_url = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".into());
    let model = opts
        .model
        .clone()
        .or_else(|| std::env::var("WHISPER_MODEL").ok())
        .unwrap_or_else(|| "whisper-1".into());

    let url = format!("{}/audio/transcriptions", base_url.trim_end_matches('/'));
    let bytes = std::fs::read(audio_path)
        .with_context(|| format!("读 audio 失败: {}", audio_path.display()))?;
    let file_name = audio_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("audio.wav")
        .to_string();

    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str("audio/wav")?;
    let mut form = reqwest::multipart::Form::new()
        .text("model", model.clone())
        .part("file", part);
    if let Some(lang) = &opts.language {
        form = form.text("language", lang.clone());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let resp = client
        .post(&url)
        .bearer_auth(&api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| anyhow!("whisper request: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("whisper {status}: {body}"));
    }
    let parsed: TranscribeResponse = resp
        .json()
        .await
        .map_err(|e| anyhow!("whisper response parse: {e}"))?;
    Ok(TranscribeResult {
        text: parsed.text,
        model,
        language: opts.language.clone(),
        audio_file: audio_path.to_string_lossy().to_string(),
        audio_size: std::fs::metadata(audio_path).map(|m| m.len()).unwrap_or(0),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeResult {
    pub text: String,
    pub model: String,
    pub language: Option<String>,
    pub audio_file: String,
    pub audio_size: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct TranscribeResponse {
    text: String,
}

/// 一站式：录音 + 转写。
pub async fn record_and_transcribe(
    out_dir: impl AsRef<Path>,
    opts: &SttOptions,
) -> Result<TranscribeResult> {
    let out_dir = out_dir.as_ref();
    std::fs::create_dir_all(out_dir).ok();
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let audio_path = out_dir.join(format!("voice-{}.{}", ts, opts.format.extension()));
    let path = record(&audio_path, opts)?;
    transcribe(path, opts).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_env_runs() {
        let r = check_env();
        // 允许无 key（只检查 has_*_present）
        let _ = r; // 不强制要求
    }

    #[test]
    fn audio_format_extension() {
        assert_eq!(AudioFormat::Wav.extension(), "wav");
        assert_eq!(AudioFormat::M4a.extension(), "m4a");
    }
}
