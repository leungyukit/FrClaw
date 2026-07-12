//! 静态文件 —— HTML / CSS / JS 内嵌到 binary。

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::body::Body;

pub const INDEX_HTML: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>fr-claw · Web 控制台</title>
<link rel="stylesheet" href="/static/style.css">
</head>
<body>
<header>
  <h1>fr-claw</h1>
  <span class="tag">v0.1.0 · Web 控制台 · Round 11</span>
  <div class="status" id="status">加载中…</div>
</header>

<main>
  <div id="messages" class="messages"></div>
  <form id="form" class="input-bar">
    <textarea id="input" rows="2" placeholder="发消息…（Enter 发送，Shift+Enter 换行）"></textarea>
    <button type="submit">发送</button>
  </form>
</main>

<script>
let TOKEN = new URLSearchParams(location.search).get("token") || localStorage.getItem("fr_token") || "";

async function authFetch(url, opts = {}) {
  if (TOKEN) {
    opts.headers = { ...(opts.headers || {}), "Authorization": "Bearer " + TOKEN };
  }
  const r = await fetch(url, opts);
  if (r.status === 401) {
    document.body.innerHTML = "<h2 style='color:red;padding:2em'>401 Unauthorized</h2><p>token 不对，请用 ?token=xxx 重连</p>";
    throw new Error("401");
  }
  return r;
}

const $msg = document.getElementById("messages");
const $input = document.getElementById("input");
const $form = document.getElementById("form");
const $status = document.getElementById("status");

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

function appendMessage(role, text) {
  const wrap = el("div", "msg msg-" + role);
  const head = el("div", "msg-head", role === "user" ? "你" : "AI");
  const body = el("div", "msg-body");
  body.textContent = text;
  wrap.appendChild(head);
  wrap.appendChild(body);
  $msg.appendChild(wrap);
  $msg.scrollTop = $msg.scrollHeight;
  return body;
}

function appendTool(name, preview) {
  const t = el("div", "tool");
  t.textContent = "🔧 " + name + (preview ? ": " + preview : "");
  $msg.appendChild(t);
  $msg.scrollTop = $msg.scrollHeight;
}

async function loadStatus() {
  try {
    const r = await authFetch("/api/status");
    const s = await r.json();
    $status.textContent = `${s.provider} · ${s.cwd} · sandbox:${s.sandbox.enabled} · rag:${s.rag.chunks} · mcp:${s.mcp.servers}/${s.mcp.tools} · skills:${s.skills.count}`;
  } catch (e) {
    $status.textContent = "状态加载失败";
  }
}

async function sendMessage(text) {
  appendMessage("user", text);
  const aiBody = appendMessage("ai", "");
  $input.value = "";
  $input.disabled = true;

  const r = await authFetch("/api/chat", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ message: text }),
  });
  const reader = r.body.getReader();
  const dec = new TextDecoder();
  let buf = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += dec.decode(value, { stream: true });
    let idx;
    while ((idx = buf.indexOf("\n\n")) >= 0) {
      const evt = buf.slice(0, idx);
      buf = buf.slice(idx + 2);
      const lines = evt.split("\n");
      let name = "message", data = "";
      for (const line of lines) {
        if (line.startsWith("event: ")) name = line.slice(7).trim();
        else if (line.startsWith("data: ")) data = line.slice(6);
      }
      if (!data) continue;
      try {
        const obj = JSON.parse(data);
        if (obj.type === "delta") aiBody.textContent += obj.content;
        else if (obj.type === "tool_calls") obj.calls.forEach(c => appendTool(c.name, ""));
        else if (obj.type === "tool_result") appendTool(obj.name, obj.preview);
        else if (obj.type === "error") aiBody.textContent += "\n[error] " + obj.message;
        else if (obj.type === "done") {/* end */}
      } catch (e) {}
      $msg.scrollTop = $msg.scrollHeight;
    }
  }
  $input.disabled = false;
  $input.focus();
}

$form.addEventListener("submit", e => {
  e.preventDefault();
  const text = $input.value.trim();
  if (text) sendMessage(text);
});

$input.addEventListener("keydown", e => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    $form.dispatchEvent(new Event("submit"));
  }
});

loadStatus();
</script>
</body>
</html>
"#;

pub const STYLE_CSS: &str = r#"
* { box-sizing: border-box; }
body {
  font-family: -apple-system, "PingFang SC", "Helvetica Neue", Arial, sans-serif;
  margin: 0; padding: 0;
  background: #1a1a1a; color: #e6e6e6;
  height: 100vh; display: flex; flex-direction: column;
}
header {
  padding: 0.6em 1.2em; background: #222;
  border-bottom: 1px solid #333;
  display: flex; align-items: center; gap: 1em;
}
header h1 { font-size: 1.2em; margin: 0; }
header .tag { font-size: 0.75em; color: #888; }
header .status { margin-left: auto; font-size: 0.8em; color: #aaa; font-family: ui-monospace, "SF Mono", monospace; }
main { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
.messages {
  flex: 1; overflow-y: auto; padding: 1em;
  display: flex; flex-direction: column; gap: 0.8em;
}
.msg { padding: 0.6em 0.9em; border-radius: 8px; max-width: 80%; }
.msg-user { background: #2a4d7a; align-self: flex-end; }
.msg-ai { background: #2d2d2d; align-self: flex-start; }
.msg-head { font-size: 0.7em; color: #aaa; margin-bottom: 0.3em; }
.msg-body { white-space: pre-wrap; font-size: 0.95em; line-height: 1.5; }
.tool {
  font-size: 0.8em; color: #999;
  padding: 0.3em 0.6em; background: #232323;
  border-left: 3px solid #555; border-radius: 4px;
  align-self: flex-start;
}
.input-bar {
  display: flex; gap: 0.6em; padding: 0.8em;
  background: #222; border-top: 1px solid #333;
}
.input-bar textarea {
  flex: 1; resize: none; padding: 0.5em 0.7em;
  background: #1a1a1a; color: #e6e6e6; border: 1px solid #444;
  border-radius: 6px; font-family: inherit; font-size: 0.95em;
}
.input-bar button {
  padding: 0.5em 1.2em; background: #2a7a4d; color: white;
  border: none; border-radius: 6px; cursor: pointer;
  font-size: 0.95em;
}
.input-bar button:hover { background: #2f8a55; }
"#;

pub async fn get_index() -> Response {
    html_response(INDEX_HTML)
}

pub async fn get_style() -> Response {
    css_response(STYLE_CSS)
}

fn html_response(s: &str) -> Response {
    let mut resp = Response::new(Body::from(s.to_string()));
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    resp
}

fn css_response(s: &str) -> Response {
    let mut resp = Response::new(Body::from(s.to_string()));
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/css; charset=utf-8"),
    );
    resp
}

pub fn _unused_ke() -> HeaderMap {
    HeaderMap::new()
}
