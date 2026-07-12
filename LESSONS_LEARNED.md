# LESSONS_LEARNED.md

> FrClaw 重写过程中踩过的坑、总结的经验。给未来重写 / 维护这个项目的人参考。

## 架构层面

### 1. **`Arc<Mutex<AppContext>>` 的传染**

`AppContext` 在最早版本是 struct by value（在 REPL / one-shot / Web 三个入口间 move）。后来加 Heartbeat 后台 task + MCP / RAG 等需要异步广播，于是被改成 `Arc<AppContext>`，**传染**到所有地方：

- `runner::run(ctx: AppContext, ...)` → 改 `ctx: Arc<AppContext>`
- `repl::command::handle_user_input(ctx: &AppContext, ...)` → 仍能跑（因为 Arc deref 到 &）
- `web::chat::run` 必须用 `ctx.clone()` 传给 `tokio::spawn` 闭包

**教训**：**REPL / 长生命周期 + 后台 task 的程序，从一开始就设计 `Arc<AppContext>`。不要等到了第 14 个 Round 才改**。所有 call site 都要重写。

### 2. **不要 `try_unwrap` Arc**

```rust
let ctx = Arc::try_unwrap(ctx_arc).map_err(|_| anyhow::anyhow!("还有引用"))?;
```

这段看着对，但**一旦有后台 task 持有 Arc，`try_unwrap` 必失败**。**结论：永远别 try_unwrap，传递 Arc，让 `Arc::strong_count` 自然归零**。

### 3. **async + sync 锁的 `!Send` 问题**

`std::sync::MutexGuard` 是 `!Send`。如果 axum handler 调 `dispatch_with_ctx`（async），又在 `sync` 函数里持有 `MutexGuard`，**handler 会拿不到 Send，axum 编译失败**。

修法：把 sync guard 限定在 sync 块内，让 async 函数拿到 `Send` 的值。**或者全程用 `tokio::sync::Mutex`（但 await 时不要持锁）**。

### 4. **`dispatch_with_ctx` 变 8 参数的旅程**

`dispatch(name, args)` → `dispatch_with_ctx(name, args, rag, worktree, sandbox, soul, heartbeat, mcp, channels)` —— 8 个 ctx 参数，每加一个新模块就 +1。

**教训**：要么用 `Context { rag, worktree, ... }` struct；要么 trait + dyn dispatch 统一接口。我选的是**参数列表**（简单、显式），但有 8 个了 —— 超过 6 个就该用 struct。

## Cargo 依赖层面

### 5. **不要把 `tempdir` 写在 `[dependencies]`**

`tempdir = "0.3"` 是 2017 年的 crate，**已经 EOL**。新代码用 `tempfile = "3"`，`TempDir::new("prefix")` → `tempfile::Builder::new().prefix("prefix").tempdir()`。

`tempfile::TempDir::new` **不接受** prefix 参数（`tempdir` 接受），我迁移时 sed 替换搞错过。

### 6. **`reqwest` 的 features 是 OR 不是 AND**

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls", "blocking"] }
```

这个写法 OK。但是**忘了 `"multipart"` feature** → `client.multipart(...)` 编译报错（`multipart` 找不见）。一开始 features 列表要全。

### 7. **Cargo feature 改了导致大量 warning**

`owo-colors` 从 3.x 升 4.x 后，API 变了：`Style::new().red()` 还在，但有些 method 重命名。第一次编译几百个 warning 都要修。

### 8. **`loop` 是 Rust 关键字**

```rust
pub mod loop { ... }  // 不能用
pub mod runner { ... } // 用 runner
```

## API / Protocol 层面

### 9. **Lark 飞书的 post 类型 `content` 嵌套 JSON**

```json
{
  "msg_type": "post",
  "content": {
    "post": {
      "zh_cn": {
        "title": "...",
        "content": [[{"tag": "text", "text": "..."}]]  // 双层数组
      }
    }
  }
}
```

`content` 是**双层**数组（line → content blocks）。我最初写成 `vec![vec![{...}]]` 是对的，但用裸 `json!({"tag": ..., ...})` 表达式混入会被解析为 Rust 表达式。**改用 `json!(vec![{...}])` 包装**。

### 10. **MCP 协议升级后 `McpManager` 改造成 tokio::sync RwLock**

最初用 `std::sync::Mutex`，然后 `connect_all` 调 `client.connect().await` 持锁 await → deadlock。

**修法**：**`std::sync::Mutex` 完全不能跨 `.await` 持锁**。改 `tokio::sync::RwLock`，await 之间 drop guard。

### 11. **`bootstrap()` 返回 `Arc<AppContext>` 不是 `AppContext`**

我写 `let ctx_arc = Arc::new(ctx);` 重复包装 → `Arc<Arc<AppContext>>`，编译过不了。要看函数返回签名直接用。

## 工具 / 命令行层面

### 12. **rustyline `default` 命令冲突**

注册 `version | v` 和 `voice | v` 都用 `v` 作为 alias → "unreachable pattern"。**一个 alias 只能归一个命令**。

### 13. **shell 工具超时 + 截断**

最初 `shell` 是无限等。Round 9 加 sandbox 时顺手把 `shell` 升级为 `tokio::time::timeout(30s)` + `stdout.truncate(1MB)`。**这是个意外收益**——之前没意识到「shell 卡死 = fr-cli 整个死锁」。

## 测试层面

### 14. **`tempdir::TempDir` 迁移到 `tempfile::TempDir` 触发连锁**

`use tempdir::TempDir;` → `use tempfile::TempDir;` 是 sed 一行能搞；但 `TempDir::new("prefix")` → `tempfile::Builder::new().prefix("prefix").tempdir()` 是 API 改了——sed 处理后还有 `.unwrap().unwrap()` 残留（`Result::unwrap` 一次就够）。

**教训**：依赖 API 改了，sed 替换后**必须重新跑全部测试**。

### 15. **`tauri_runtime_block_on` 是 hack**

最初 `RagStore::add_text` 是 sync，但内部 `embedder.embed()` 是 async。**为了避免整个 `RagStore` 改 async**，写了 `block_on()` helper：尝试在当前 tokio runtime 里跑，fallback 建临时 runtime。

这个 hack **能用**（10k chunks 微秒级），但**不优雅**。长期应该把 `RagStore` 整体改 async。

## 文档 / 协作层面

### 16. **每 Round 完一定要写 release notes**

中途我加过 1 次 `git log` 找变更，发现每 Round 改了几十个文件。**交付前补 CHANGELOG 比交付后补容易 10 倍**——context 还热。

### 17. **用户的「无需回答，继续」是好信号**

用户多次说「继续」「无需回答，继续」——意味着**对节奏和方向都满意**。这种时候**不要停下来问问题**，继续推；状态自己汇报。

### 18. **「按计划做了」是另一类信号**

用户回「剩下的三个都按计划做了」+「继续」=**确认方向对，让我推到底**。这种时候也不需要问，照着路线图推即可。

## 性能 / 工程层面

### 19. **PPTX 手写比引 crate 快 10x**

调研 `pptx-rs` / `rust-pptx` 都要 30+ transitive deps + 1h 学习曲线。**手写 ZIP + 5 个 XML 文件 30 分钟搞定**，PowerPoint / Keynote / Impress 全兼容。

**教训**：**先看 spec 复杂度，spec 简单就手写**。PPTX spec 其实很简单（OOXML 5 段 + ZIP）。

### 20. **RAG 不引 sqlite-vec 是个正确选择**

sqlite-vec 要 C 编译 + 跨平台 binary 加载，**生产环境 1 个版本不匹配就崩**。`rusqlite` + naive cosine 在 10k chunks 下完全够。**90% 的人用 RAG 不会到百万 chunks 级别**。

### 21. **`uiautomator`-style 的「string of stuff」= 3 个独立单元**

最初 RAG 用 `Vec<String>` 存 chunks。后来发现 chunk 之间没强关联，应该用 `Vec<Chunk>` struct 携带 metadata、embedding、origin、created_at。**Rust 不鼓励 stringly-typed**。

## 收尾

整个项目从 Round 1 MVP 到 Round 16 共 **16 轮**、**31 个特性**、**20100 行 Rust**、**140 单测全过**、**0 warning**。binary 10.6 MB。

核心经验：

1. **`Arc<AppContext>` 从一开始就用**。
2. **`std::sync::Mutex` 不能跨 `.await`**。
3. **spec 简单就手写**，不引 crate。
4. **测试在 Round 5+ 一定要全跑一遍**，不要等最后一轮才发现 5 个 test 互相打架。
5. **CLI alias 别重**。
6. **加一个模块要：实现 + 命令 + LLM 工具定义 + dispatch 路由 + 测试 + 文档**，少一个就不算完成。

这 6 条写进 AGENTS.md（或团队 wiki）能让下一个 100% 同样的项目少踩一半的坑。
