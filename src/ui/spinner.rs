//! 等待 LLM 响应时的终端加载符（spinner）。
//!
//! 在独立线程上往 **stderr** 轮询刷帧（`|/-\\`），不污染 stdout 的正文输出。
//! - 首次拿到 token / 工具调用时调用方 `stop()`
//! - `Drop` 兜底：即使出错 / panic 也会清掉残行

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const FRAMES: &[char] = &['|', '/', '-', '\\'];

pub struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    /// 是否实际启动了动画（非 TTY 环境为 false，stop 时无需清行）
    active: bool,
}

impl Spinner {
    /// 启动加载符。非 TTY（管道 / 重定向）环境下不画动画，接近零开销。
    pub fn start(msg: &str) -> Self {
        let active = std::io::stderr().is_terminal();
        if !active {
            return Self { stop: Arc::new(AtomicBool::new(true)), handle: None, active };
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stop_c = Arc::clone(&stop);
        let msg = msg.to_string();
        let handle = std::thread::spawn(move || {
            let mut i = 0usize;
            while !stop_c.load(Ordering::Relaxed) {
                eprint!("\r{} {}", FRAMES[i % FRAMES.len()], msg);
                let _ = std::io::stderr().flush();
                i += 1;
                std::thread::sleep(Duration::from_millis(80));
            }
        });
        Self { stop, handle: Some(handle), active }
    }

    /// 停止动画并清掉 spinner 行。可重复调用（幂等）。
    pub fn stop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        // 擦除 spinner 残留行：回车 + 清行 + 回车
        eprint!("\r\x1b[2K\r");
        let _ = std::io::stderr().flush();
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_is_idempotent_and_nonblocking() {
        let mut sp = Spinner::start("test");
        sp.stop();
        sp.stop(); // 不应 panic / 挂起
    }

    #[test]
    fn drop_without_explicit_stop_is_safe() {
        let sp = Spinner::start("test");
        drop(sp);
    }
}
