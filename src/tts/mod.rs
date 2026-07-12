//! Round 15d ─ TTS（macOS `say` + `afconvert`）。
//!
//! 策略：
//! 1. `say -o <aiff> -v <voice> <text>` ── 产 .aiff
//! 2. `afconvert -f mp4f -d aac <aiff> <m4a>` ── 转 .m4a
//!
//! 两者都是 macOS 自带，零依赖。Linux/Windows 暂时返回「unsupported platform」。

use anyhow::{anyhow, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct TtsOptions {
    /// `say -v` 用的 voice。`None` 用系统默认。
    pub voice: Option<String>,
    /// 速率（words per minute），say 默认 ~175
    pub rate: Option<u32>,
    /// 目标格式：aiff / m4a
    pub format: TtsFormat,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TtsFormat {
    #[default]
    M4a, // AAC
    Aiff,
}

/// 检查当前平台是否支持。
pub fn is_supported() -> bool {
    cfg!(target_os = "macos")
}

/// 列出可用 voice（macOS）。
pub fn list_voices() -> Result<Vec<String>> {
    if !is_supported() {
        return Ok(Vec::new());
    }
    let out = Command::new("say")
        .arg("-v")
        .arg("?")
        .output()
        .map_err(|e| anyhow!("say not found: {e}"))?;
    if !out.status.success() {
        return Err(anyhow!("say -v ? failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter_map(|line| line.split_whitespace().next().map(|s| s.to_string()))
        .collect())
}

/// 合成一段文字到文件。
pub fn synthesize(text: &str, out_path: impl AsRef<Path>, opts: &TtsOptions) -> Result<()> {
    if !is_supported() {
        return Err(anyhow!(
            "TTS 只在 macOS 支持（用 `say` + `afconvert`）。当前 platform: {}",
            std::env::consts::OS
        ));
    }
    let out_path = out_path.as_ref();
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // 1) say 出 AIFF（中间文件，放在 out_path 旁）
    let aiff_path = match opts.format {
        TtsFormat::Aiff => out_path.to_path_buf(),
        TtsFormat::M4a => {
            let mut p = out_path.to_path_buf();
            p.set_extension("aiff");
            p
        }
    };

    let mut cmd = Command::new("say");
    cmd.arg("-o").arg(&aiff_path);
    if let Some(v) = &opts.voice {
        cmd.arg("-v").arg(v);
    }
    if let Some(r) = opts.rate {
        cmd.arg("-r").arg(r.to_string());
    }
    cmd.arg(text);

    let status = cmd
        .status()
        .map_err(|e| anyhow!("say failed to spawn: {e}"))?;
    if !status.success() {
        return Err(anyhow!("say 退出非 0: {status}"));
    }

    if opts.format == TtsFormat::Aiff {
        return Ok(());
    }

    // 2) afconvert aiff → m4a
    let status = Command::new("afconvert")
        .arg("-f").arg("mp4f")     // mp4 container
        .arg("-d").arg("aac")      // AAC codec
        .arg(&aiff_path)
        .arg(out_path)
        .status()
        .map_err(|e| anyhow!("afconvert failed to spawn: {e}"))?;
    if !status.success() {
        return Err(anyhow!("afconvert 退出非 0: {status}"));
    }

    // 3) 清理中间 aiff
    let _ = std::fs::remove_file(&aiff_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_supported_matches_platform() {
        assert_eq!(is_supported(), cfg!(target_os = "macos"));
    }

    #[test]
    fn list_voices_returns_or_empty() {
        if is_supported() {
            let v = list_voices().unwrap();
            assert!(!v.is_empty());
            // macOS 默认带 Tingting / Sin-ji / Mei-Jia 之一
            assert!(v.iter().any(|n| n.contains("Ting") || n.contains("Sin") || n.contains("Mei") || n.contains("en")));
        }
    }
}
