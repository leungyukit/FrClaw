//! Token 上下文压缩 —— 长会话自动摘要早期对话。
//!
//! 策略：
//! - 「消息条数」超阈值（默认 `history_window * 8`）时，把最早 N 条 user/assistant 消息
//!   抽出来，浓缩成一段「摘要」，并以 `system` 角色内嵌进 session。
//! - 同时把 system prompt 里原有的「项目记忆」内容保留。
//! - 不调 LLM：直接启发式把早期 N 条内容拼成 raw 摘要（前 N 条）插入。
//!   下次想升级再改成「调用 LLM 真正摘要」。

use crate::session::chat::ChatSession;

/// 当 `messages.len() > threshold` 时，把最老的 `compact_count` 条对话抽出来做成摘要段。
pub fn maybe_compact(session: &mut ChatSession, threshold: usize, compact_count: usize) {
    // 总消息条数 = system(1) + 历史。
    if session.messages.len() <= threshold + 1 {
        return;
    }
    if compact_count == 0 || compact_count + 1 >= session.messages.len() {
        return;
    }

    // 保护：保留最新 N 条对话摘要不动。
    let mut buf = String::new();
    let mut taken = 0usize;
    // 从 1 开始（系统消息不动）
    let mut i = 1;
    while i + compact_count < session.messages.len() - 1 && taken < compact_count {
        let m = &session.messages[i];
        let role = match m.role {
            // 跳过 system 消息（如已插入的摘要段），但必须推进 i，否则会死循环
            crate::llm::message::Role::System => {
                i += 1;
                continue;
            }
            crate::llm::message::Role::User => "user",
            crate::llm::message::Role::Assistant => "assistant",
            crate::llm::message::Role::Tool => "tool",
        };
        if !m.content.trim().is_empty() {
            buf.push_str(&format!("- [{role}] {}\n", m.content.replace('\n', " ").chars().take(200).collect::<String>()));
        }
        taken += 1;
        i += 1;
    }

    if taken == 0 {
        return;
    }

    // 找到现有摘要段（系统 prompt 第二个 system message）替换；否则插入新的。
    let summary = format!(
        "\n# 早期对话摘要（自动压缩，{} 条）\n{}\n",
        taken, buf
    );

    // 找到第二条 system 消息（如果有）；否则在第一条之后插入。
    let insert_idx = session
        .messages
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, m)| matches!(m.role, crate::llm::message::Role::System))
        .map(|(idx, _)| idx);

    match insert_idx {
        Some(idx) => {
            // 把已有摘要段扩充
            let mut msg = session.messages[idx].clone();
            msg.content.push_str(&summary);
            session.messages[idx] = msg;
        }
        None => {
            // 插入新 system message
            session.messages.insert(
                1,
                crate::llm::message::Message::system(summary),
            );
        }
    }

    // 移除被抽走的对话（cut_at 之前不包含 system）
    // 简化：保留系统提示 + 摘要段 + 后续 N 条；中间全删
    let system_count = session
        .messages
        .iter()
        .take_while(|m| matches!(m.role, crate::llm::message::Role::System))
        .count();
    let keep_tail = session.messages.len().saturating_sub(compact_count + system_count);
    let mut new_msgs: Vec<crate::llm::message::Message> = session.messages[..system_count].to_vec();
    new_msgs.extend(session.messages[keep_tail..].iter().cloned());
    session.messages = new_msgs;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::message::{Message, Role};

    fn build_long_session(n: usize) -> ChatSession {
        let mut s = ChatSession::new(
            "test",
            &crate::config::models::builtin_default(),
            &crate::config::settings::Settings::default(),
            "default",
        );
        for i in 0..n {
            s.messages.push(Message::user(format!("msg#{i}")));
            s.messages.push(Message::assistant(format!("reply#{i}")));
        }
        s
    }

    #[test]
    fn compact_when_too_long() {
        let mut s = build_long_session(40);
        let original_len = s.messages.len();
        maybe_compact(&mut s, 20, 10);
        assert!(s.messages.len() < original_len);
        // 早期对话应被压成一条 system summary
        assert!(s.messages.iter().any(|m| matches!(m.role, Role::System) && m.content.contains("摘要")));
    }

    #[test]
    fn compact_again_with_existing_summary_terminates() {
        // 第一次压缩后 messages[1] 会变成 system（摘要段）；
        // 再次压缩时必须跳过它而不是死循环。
        let mut s = build_long_session(60);
        maybe_compact(&mut s, 32, 16);
        assert!(matches!(s.messages[1].role, Role::System));
        // 模拟后续对话，让消息数再次超过阈值
        for i in 0..20 {
            s.messages.push(Message::user(format!("more#{i}")));
            s.messages.push(Message::assistant(format!("resp#{i}")));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            maybe_compact(&mut s, 32, 16);
            tx.send(()).unwrap();
        });
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("maybe_compact 死循环");
    }

    #[test]
    fn no_compact_when_short() {
        let mut s = build_long_session(10);
        let len_before = s.messages.len();
        maybe_compact(&mut s, 20, 10);
        assert_eq!(len_before, s.messages.len());
    }
}
