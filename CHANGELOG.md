# Changelog

> FrClaw 变更日志。从 Round 1 MVP 推到 Round 16（31 个特性 / 140 单测 / 0 warning），后改名 FrClaw。

## v0.2.1 ─ LLM 超时策略修复（2026-09-19）

### 修复

- **长生成被误杀**：HTTP 客户端总超时 180s 会掐断持续数分钟的流式生成（推理模型 + 工具循环场景必现 `operation timed out`）
  - 总超时放宽为 1800s 安全网（`FR_LLM_TIMEOUT_SECS` 可覆盖）
  - 新增流内空闲超时：相邻数据块间隔 >120s 才判定卡死（`FR_LLM_IDLE_TIMEOUT_SECS` 可覆盖）

### 验证

- `cargo test`：162/162 pass
- 真实 API 端到端验证

---

## v0.2.0 ─ 流式输出与稳定性修复（2026-09-19）

### 新增

- **真实流式输出**：`run_agent_step_loop` 改为消费 `chat_stream`，每个 delta 立即上报事件；REPL 经 `MarkdownStream` 逐行增量渲染，Web 控制台按 `delta` SSE 推送
- **等待加载符**：新增 `ui::spinner`，等待首个 token 时显示旋转动画（stderr 输出、不污染正文、非 TTY 自动关闭、Drop 兜底清行）
- **provider 明文凭据**：`ProviderConfig` 新增 `api_key` 字段（优先级低于 `api_key_env`），缺 key 时在构建期明确报错

### 修复

- **SSE 解析死循环**（所有 LLM 请求必现）：按行切分时未消费换行符，残留 `\n` 导致 100% CPU 空转、`chat_stream` 永不返回
- **模型切换不生效**：`/model` 只改字符串、降级链缓存不重建，请求仍发给旧 provider；切换时现在构建新 provider 并热注入 `FallbackChain`（`/key` 同步热更新）
- **长会话压缩死循环**：二次压缩遇到已插入的摘要 system 消息时 `continue` 未推进索引
- **流式渲染 panic**：增量 diff 用旧渲染字节长度切新字符串，CJK 多字节字符边界处 panic；改为最长公共前缀（字符边界安全）；单行强制 flush 截断点同样推进到字符边界
- **回复重复输出**：流式内容已逐行渲染，结尾不再整体重打

### 验证

- `cargo test`：162/162 pass（含 SSE 切分、压缩回归、spinner、字符边界新增测试）
- 真实 API 端到端：volcengine / deepseek 流式对话、中文 markdown、模型切换

---

## v0.1.2 ─ /config 重构与模型配置挂起修复（2026-07-13）

### 新增

- `/config` 进入交互式配置菜单，支持：
  - **模型**（model）── 向导式添加/配置 LLM provider
  - **通道**（channel）── 向导式配置飞书/钉钉/企微/Webhook，保存后热重载
  - **思考模式**（mode）── direct / CoT / ToT / ReAct / Plan
  - **语言**（lang）── zh / en，自动重建 system prompt
  - **自治模式**（autonomous）── 工具授权自动确认开关
  - **单轮限制**（limit）── max_tokens 设置
- 保留快捷入口：`/config model`、`/config channel`、`/config mode` 等。

### 修复

- 修复模型配置后「像死机」的挂起问题：
  - Heartbeat、`/repl/command.rs`、`/repl/runner.rs` 中 `chain.read()` 不再跨 `.await` 持有。
  - `OpenAiCompatProvider` 增加 15 秒连接超时，避免不可达 base_url 长时间挂起。
  - `ChannelManager` 改为 `Arc<RwLock<...>>`，支持通道配置热重载。

### 验证

- `cargo build` 通过
- `cargo test`：140/140 pass

---

## Round 17 ─ 改名 fr-cli-rs → FrClaw（2026-07-12）

### 改动

- `Cargo.toml` `name = "fr-cli-rs"` → `name = "fr-claw"`
- Binary 名字 `fr` 保留（短、易敲、向后兼容）
- ASCII art banner 改成 `F r C l a w`
- 源代码 / 注释 / 测试样本字符串 / 文档（README / CHANGELOG / LESSONS_LEARNED）统一替换
- `fr-cli` 引用「Python 上游项目」时**保留**；指代「本项目」时全部改 `FrClaw` / `fr-claw`
- `~/.fr_cli/` 用户配置目录 / `mcp_servers.json` / `channels.json` 等用户文件**保留路径**（不破坏用户现有配置）

### 验证

- `cargo build` 通过：0 warning
- `cargo test --lib`：140/140 pass
- Binary 名仍是 `fr`，所有命令 / 配置文件 / 行为不变

### 未做（保留 Python fr-cli 指代）

README 里提到「Python 版 fr_cli」/「Rust 重制版」是历史指代上下文，保留。

---

## Round 16 ─ 集成 & 媒体补全（2026-07-12）

### 新增

- **多通讯通道**（`channels/`）—— 4 个 channel 共用 `Channel` trait：
  - 飞书（`lark.rs`）── HmacSHA256 加签、text + post 富文本
  - 钉钉（`dingtalk.rs`）── HmacSHA256 加签、text + markdown
  - 企业微信（`wecom.rs`）── text + markdown（自动截断 4096 字节）
  - 通用 Webhook（`webhook.rs`）── POST JSON 到任意 URL
  - `ChannelManager`（`registry.rs`）── 多 channel + send / broadcast
  - 持久化 `~/.fr_cli/channels.json`（`config.rs`）
- **Timeline HTML 导出**（`export/timeline_html.rs`）── 自包含 HTML（无外链）、时间线布局、暗/亮色自适应、点 card 复制内容
- **Voice 录制 + STT**（`voice/mod.rs`）── sox / rec / ffmpeg → OpenAI Whisper API

### 命令

- `/channels list / add / remove / toggle / test / broadcast / init / path`
- `/timeline session <name> [out.html] [--theme dark|light|auto]`
- `/timeline demo [out.html]`
- `/voice check / record / transcribe / say`

### LLM 工具增量

- `channel_send` / `channel_broadcast`（2 个）

### 统计

140 tests pass, 0 warning, 104 .rs, 20100 行, binary 10.6 MB。

---

## Round 15 ─ 协议 & 媒体补全（2026-07-12）

### 新增

- **MCP Resources / Prompts API**（`mcp/`）── 补全 5,700+ 工具生态的另外 2 大 API：`refresh_resources` / `read_resource` / `refresh_prompts` / `get_prompt`
- **RAG 混合检索**（`rag/hybrid.rs`）── BM25 + vector cosine 加权融合，256-bucket token hash，`rag_query` 工具默认走 hybrid
- **PPTX 导出**（`export/mod.rs` + `slides.rs`）── 手写 OOXML + ZIP（不引 `pptx` crate）
- **TTS**（`tts/mod.rs`）── macOS `say` + `afconvert` 包装

### 命令

- `/mcp resources / read-resource / prompts / get-prompt / refresh`
- `/pptx session / heartbeat / demo`
- `/tts voices / say / session`
- `/rag query [--hybrid|--vector] [--vec-w 0.5]`

### LLM 工具增量

`mcp_list_resources` / `mcp_read_resource` / `mcp_list_prompts` / `mcp_get_prompt`（4 个）

### 统计

124 tests pass, 0 warning, 96 .rs, 17200 行。

---

## Round 14 ─ Heartbeat 主动唤醒

### 新增

- `heartbeat/policy.rs` ── 从 SOUL.md `## heartbeat` 段解析
- `heartbeat/state.rs` ── `~/.fr_cli/heartbeat_state.json` 持久化 + history（最多 50 条）
- `heartbeat/registry.rs` ── 后台 tick 协程（1 分钟）+ `run_now` 手动触发
- `heartbeat/runner.rs` ── 一次 heartbeat 跑（调 LLM、收集 tool calls、写 long-term 报告）
- `tools/heartbeat.rs` ── LLM 可见 3 工具（status / now / set）

### 命令

`/heartbeat status / on / off / now / interval / directives [list|add|clear] / history / reload`

### 统计

114 tests pass, 0 warning, 88 .rs, 16044 行。

---

## Round 13 ─ SOUL.md 持久身份

### 新增

- `soul/loader.rs` ── `SoulContent::load(cwd)` 多源合并
- `tools/soul.rs` ── LLM 可见 `read_soul` / `append_soul` 工具
- 启动期注入 SOUL 到 system prompt 末尾

### 命令

`/soul show / path / edit / append / init / reload`

### 统计

102 tests pass, 0 warning, 83 .rs, 14800 行。

---

## Round 12 ─ Hermes 后台任务引擎

### 新增

- `hermes/task.rs` ── Task / TaskRun / TaskKind / TaskStatus
- `hermes/store.rs` ── sqlite 持久化 + 状态机
- `hermes/cron.rs` ── 5-field cron 解析器（自写，不引 `tokio-cron-scheduler`）
- `hermes/worker.rs` ── 后台 worker：执行 task + cron 调度
- `hermes/registry.rs` ── 全局 registry + 后台 tick

### 命令

`/tasks list / add / show / run / approve / reject / remove / history` + `/cron validate / add / list / remove / fire`

### 统计

96 tests pass, 0 warning, 80 .rs, 14300 行。

---

## Round 11 ─ Web 控制台

### 新增

- `web/server.rs` ── axum server + router
- `web/api.rs` ── `/api/{status,skills,sandbox,mcp,rag,command}` + 各模块状态
- `web/chat.rs` ── SSE 流式 chat endpoint
- `web/auth.rs` ── Bearer token 生成 + 校验
- `web/static_files.rs` ── 内嵌 HTML + CSS
- `cli/args.rs` ── `web` subcommand

### 入口

`./fr web [--port 8765] [--host 127.0.0.1] [--no-auth]`

### 统计

81 tests pass, 0 warning, 74 .rs, 12900 行。

---

## Round 10 ─ Streaming Markdown 增量渲染

### 新增

- `ui/markdown_stream.rs` ── 行级 flush 增量渲染器（代码块、标题、列表、引用、bold、italic、链接、200 字符强 flush 阈值）
- 集成到 `repl/command.rs` 的 LLM 流式输出回调

### 统计

70 tests pass, 0 warning, 67 .rs, 11800 行。

---

## Round 9 ─ Sandbox 隔离

### 新增

- `sandbox/policy.rs` ── 路径/命令黑/白名单 + 持久化
- `sandbox/check.rs` ── 路径/命令检查 + `SandboxVerdict`
- `sandbox/macos.rs` ── macOS `sandbox-exec` scheme
- 集成到 `read_file` / `write_file` / `list_dir` / `shell` 工具
- 额外把 `shell` 工具升级为带**超时 + stdout 截断**的执行

### 命令

`/sandbox on / off / check / allow / deny / status / test`

### 统计

70 tests pass, 0 warning, 67 .rs, 11800 行。

---

## Round 8 ─ Worktree + 原子 multi_edit

### 新增

- `tools/worktree.rs` ── LLM 可见 4 工具（create / list / remove / status）
- `tools/multi_edit.rs` ── 原子 multi_edit，per-file snapshot + 全局回滚
- 自实现 `git worktree` 子进程调用（不引 libgit2）

### 命令

`/worktree list / create / use / remove / status` + `/multi_edit`

### 统计

54 tests pass, 0 warning, 63 .rs, 10900 行。

---

## Round 7 ─ RAG 个人知识库

### 新增

- `rag/chunk.rs` ── 段落 / 句子 / 窗口切分
- `rag/embed.rs` ── Embedder 抽象（provider + hash baseline 256-dim + FallbackEmbedder）
- `rag/store.rs` ── sqlite + naive cosine（不引 sqlite-vec）
- `tools/rag.rs` ── LLM 可见 4 工具 + auto-recall

### 命令

`/rag add / query / list / import / remove / status / config / auto`

### 统计

41 tests pass, 0 warning, 61 .rs, 9800 行。

---

## Round 6 ─ MCP Streamable HTTP client

### 新增

- `mcp/protocol.rs` ── JSON-RPC 2.0 envelope + Tool + CallToolResult
- `mcp/client.rs` ── Streamable HTTP 传输（POST + Mcp-Session-Id header）
- `mcp/config.rs` ── `~/.fr_cli/mcp_servers.json` schema
- `mcp/registry.rs` ── 多 server 协调 + 工具暴露
- 自实现，不引第三方 MCP crate

### 命令

`/mcp list / add / remove / reconnect / tools / call / config`

### LLM 工具

`mcp__<server>__<tool>` 自动注入

### 统计

26 tests pass, 0 warning, 56 .rs。

---

## Round 5 ─ Skills 系统

### 新增

- `skills/loader.rs` ── SKILL.md 解析（YAML frontmatter + markdown body）
- `skills/registry.rs` ── 扫 `~/.fr_cli/skills/*/SKILL.md` + builtin
- `skills/matcher.rs` ── 3 种 trigger 匹配（`@alias` / regex / substring）
- 集成到 user prompt（命中的 skill body 拼到 user 消息尾）

### 命令

`/skill list / show / dir / reload / path`

### 预装 skill

- `web-research` ── 查 + 摘要 + 写长期记忆
- `project-overview` ── 自动摸项目结构

### 统计

23 tests pass, 0 warning, 51 .rs。

---

## Round 4 ─ Hooks 系统

### 新增

- 4 类事件：`PreToolUse` / `PostToolUse` / `UserPromptSubmit` / `SessionStart`
- `hooks/events.rs` ── HookEvent / HookInput / HookOutput
- `hooks/config.rs` ── matcher（regex + fallback substring）
- `hooks/runner.rs` ── `sh -c` 跑 hook，stdout envelope 协议

### stdout envelope 协议

```json
{"additional_context": "..."}
{"modified_args": {"path": "/safe/file"}}
{"modified_prompt": "..."}
{"continue_": false, "reason": "..."}
```

### 统计

15 单测覆盖集成点，0 warning。

---

## Round 3 ─ 自我记忆进化（web 搜索版）

### 新增

- `memory/evolution.rs` ── 进化引擎：web 搜索 → 摘要 → 写盘
- `memory/recall.rs` ── 长期记忆索引
- `tools/web_search.rs` ── DuckDuckGo scraper + Brave Search API
- `tools/memorize.rs` ── memorize / recall 工具

### 命令

`/memory_topics add / rm / list` + `/memory_evolve once / auto on/off`

### 核心

web 搜索自我进化路径 + 用户对话路径双路并行。

### 统计

12 单测，5454 行。

---

## Round 2 ─ 9 个高级特性

| # | 特性 | 模块 |
|---|---|---|
| 1 | 真 SSE 流式响应 | `llm/openai_compat.rs` |
| 2 | Tool Calls 自动对接 | `tools/registry.rs` |
| 3 | MasterAgent ReAct 循环 | `agent/loop_runner.rs` |
| 4 | Plan mode | `tools/plan.rs` |
| 5 | 并行工具调用 | `agent/loop_runner.rs`（`JoinSet`） |
| 6 | /mode 思维模式 | `agent/prompts.rs` |
| 7 | 项目记忆自动加载 | `memory/project.rs` |
| 8 | Token 上下文压缩 | `memory/compressor.rs` |
| 9 | Sub-agent 委派 | `tools/subagent.rs` |

### 统计

9 单测，4370 行。

---

## Round 1 ─ MVP

### 核心

- REPL（rustyline 14 with file history）
- 6 OpenAI 兼容 provider（zhipu / deepseek / openai / moonshot / anthropic / ollama）
- Fallback chain 自动降级
- 22 内置命令
- Session 持久化（`~/.fr_cli/sessions/*.json`）
- `models.yaml` 启动期自动生成

### 统计

2876 行。

---

## 累计

| 项 | 数 |
|---|---|
| 已完成 Round | 16 |
| 累计特性 | 31 项 |
| 累计 .rs 文件 | 104 |
| 累计行数 | 20,100 |
| 单测 | 140 pass |
| Release binary | 10.6 MB |
| 0 warning | ✓ |

## 剩余路线图（未做）

- **Skill 市场协议** ── 让别人能 publish / install 远程 skill
- Telegram / Discord / Slack / Signal 通道（用户已说暂缓）
