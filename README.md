# FrClaw

> 终端 AI 助手 / AI 编程伙伴。[Python 版 fr_cli](../github_fr-cli/fr-cli) 是 4.5w 行 / 40+ 高级特性的庞然大物；
> **FrClaw** 是它的 Rust 重制版（对标 OpenClaw 生态）——目标是把核心体验做到能跑、可用、好改，把最具战力的高级特性也带过来。

## 这一版的状态

| 范围 | 状态 |
|---|---|
| MVP + 核心特性（22 内置命令 / REPL / 多 provider / 降级链 / 会话持久化） | ✅ Round 1 |
| **(1) 真 SSE 流式响应** | ✅ Round 2 |
| **(2) Tool Calls 自动对接**（read/write/shell/list_dir） | ✅ Round 2 |
| **(3) MasterAgent ReAct 循环** | ✅ Round 2 |
| **(4) Plan mode**（AI 提议计划 + 用户审批 + exit 自动跑） | ✅ Round 2 |
| **(5) 并行工具调用**（多个 tool_calls 在同 step 并发） | ✅ Round 2 |
| **(6) /mode 思维模式**（`direct / cot / tot / react / plan`） | ✅ Round 2 |
| **(7) 项目记忆自动加载**（`.frcli.md / AGENTS.md / CLAUDE.md / .github/AGENTS.md`） | ✅ Round 2 |
| **(8) Token 上下文压缩**（超阈值自动摘要老轮） | ✅ Round 2 |
| **(9) Sub-agent 委派**（`spawn_agent` / `task_output`） | ✅ Round 2 |
| **(10) 自我记忆进化（web 搜索版）** ⭐ Round 3 ─ 不靠用户对话也能自我进化 | ✅ Round 3 |
| **(11) `web_search` 工具**（LLM 可调，DuckDuckGo 默认 / Brave 可配） | ✅ Round 3 |
| **(12) `memorize` / `recall` 工具**（LLM 主动沉淀 + 召回） | ✅ Round 3 |
| **(13) Hooks 系统**（`PreToolUse` / `PostToolUse` / `UserPromptSubmit` / `SessionStart`） | ✅ Round 4 |
| **(14) `exit 2` 阻止 + stdout JSON envelope 改 tool_args / prompt / 上下文注入** | ✅ Round 4 |
| **(15) Skills 系统**（`~/.fr_cli/skills/*/SKILL.md`，自动 trigger 注入） | ✅ Round 5 |
| **(16) MCP Streamable HTTP client**（接 5,700+ 现成 MCP 工具生态） | ✅ Round 6 |
| **(17) RAG 个人知识库**（sqlite + 256-dim hashing embedder，auto-recall） | ✅ Round 7 |
| **(18) Git worktree + 原子 multi_edit** | ✅ Round 8 |
| **(19) Sandbox 隔离**（路径/命令检查 + macOS sandbox-exec 增强） | ✅ Round 9 |
| **(20) Streaming Markdown 增量渲染**（行级 flush + 强 flush 阈值） | ✅ Round 10 |
| **(21) Web 控制台**（`./fr web`，axum HTTP + Bearer token + SSE chat） | ✅ Round 11 |
| **(22) Hermes 后台任务引擎**（sqlite 持久队列 + 5-field cron + 审核流） | ✅ Round 12 |
| **(23) SOUL.md 持久身份**（多源合并 + LLM 工具 + 自动注入 system prompt） | ✅ Round 13 |
| **(24) Heartbeat 主动唤醒**（SOUL 段驱动 + 1 分钟检查 + `## heartbeat` 段 directives） | ✅ Round 14 |
| **(25) MCP Resources / Prompts 协议**（补全 5,700+ 工具生态的另外 2 大 API） | ✅ Round 15a |
| **(26) RAG 混合检索**（vector cosine + BM25 关键词加权融合） | ✅ Round 15b |
| **(27) PPTX 导出**（手写 OOXML，session / heartbeat / demo） | ✅ Round 15c |
| **(28) TTS**（macOS `say` + `afconvert` → m4a） | ✅ Round 15d |
| **(29) 多通讯通道**（飞书 / 钉钉 / 企微 / 通用 Webhook） | ✅ Round 16a-d |
| **(30) Timeline HTML 导出**（session → 自包含可分享网页） | ✅ Round 16e |
| **(31) Voice 录制 + STT**（sox / ffmpeg + OpenAI Whisper） | ✅ Round 16f |
| **(21) Web 控制台**（axum + Bearer Token + SSE 推送） | ✅ Round 11 |
| **(22) Hermes 后台任务引擎**（持久队列 + cron + 审核） | ✅ Round 12 |
| **(23) SOUL.md 持久身份**（多源合并 + 注入 system + 工具） | ✅ Round 13 |
| **对标 OpenClaw（完整 1:1）** | ❌ 路线图，单次会话不可行 |

代码量：**~14800 行 Rust**，83 个 .rs 文件，编译产物 **9.3 MB**。

102 个单元测试通过（hooks 6 + skills 9 + MCP 3 + RAG 15 + worktree 4 + multi_edit 9 + sandbox 16 + md_stream 6 + web auth 5 + hermes 15 + soul 6 + 上下文压缩 / markdown / 进化路径）。

## 关于 OpenClaw 完全 1:1 的现实

[OpenClaw](https://github.com/openclaw/openclaw)（OpenClaw = Peter Steinberger 的本地优先 AI Agent 网关，150k⭐ / 12w 行 TS / 50+ 渠道 / 75+ 原生工具 / 5,700 Skills / ClawHub / SOUL.md / MemOS / Sandbox / Cron / Web Console）的完整 1:1 不可能在一次会话内做出来——它是 900+ 贡献者两年迭代的生态。

**更现实的对标是 Claw 家族的 Rust 兄弟**：
- **ZeroClaw**（Rust 4.6k⭐）—— 3.4 MB binary / 22+ providers / SQLite hybrid memory with vector search / 1,017 tests / 3 自主等级
- **ZeptoClaw**（Rust ~4MB）—— 17 工具 / 5 渠道 / container isolation / agent swarms / 1,300+ tests / 提示注入检测 / secret 泄漏扫描 / SSRF / policy engine

FrClaw 当前在「**功能子集**」水准确认到 Round 3，下面的路线图以这俩为目标最终态。

## 这一版做的高级特性（怎么用）

### (1) 流式响应 (SSE)

直接调用 `provider.chat_stream(req)` 拿 `Stream<Item = StreamEvent::Delta | Done>`。
SSE parser 处理跨 chunk 的 data: 行；tool_calls 增量累积；usage token 单独发到最后一个 chunk。

不需要额外开关——`fr` 启动后对话自动用流式。

### (2) + (3) Tool Calls 与 MasterAgent

`agent::loop_runner::run_agent_step_loop` 跑 ReAct 循环：
```
对话 → LLM ─→ [tool_calls]
            ├─► 授权检查（autonomous=false 时弹 y/A/f/N）
            ├─► 并发 run（tokio JoinSet）
            └─► 把 tool 结果回填为 tool messages，进入下一轮
```
最多跑 10 步；空 tool_calls 结束；达到上限返回 `finish_reason="length"`。

LLM 端可见的工具集：
- `read_file(path)` / `write_file(path, content)` / `list_dir(path)` / `shell(cmd, cwd?)`

### (4) Plan mode

LLM 可以调：
- `enter_plan_mode({steps:[...]})` —— 暂停，渲染步骤，让用户审批（y/n）
- `exit_plan_mode({approved:true|false})` —— 离开

`/mode plan` 把这个变成默认行为：任何 ≥3 步的任务，AI 都会先出 plan。

### (5) 并行工具调用

LLM 一次返回 N 个 `tool_calls` 时，agent loop 用 `JoinSet::spawn` 并发执行；
所有完成后再把结果回填给模型。失败的隔离，不会影响其他 tool。

### (6) /mode

```
/mode            # 看当前 / 列举可选项
/mode direct     # 单段回答
/mode cot        # 先列出推理再回答 (Chain-of-Thought)
/mode tot        # 多分支探索后选最优 (Tree-of-Thoughts)
/mode react      # 优先调工具获取真实数据 (默认)
/mode plan       # 先 enter_plan_mode 让用户审批
```

切换后立即影响 system prompt；不需要重启 session。

### (7) 项目记忆自动加载

启动时从 cwd 向上找（不再爬上 `~`），优先命中第一个：
1. `.frcli.md`
2. `AGENTS.md`
3. `CLAUDE.md`
4. `.github/AGENTS.md`

命中后作为 system prompt 的尾段注入（`# 项目记忆（自动加载自 <path>）`）。

实时命令：
- `/memory show` —— 打印当前项目记忆
- `/memory reload` —— 重发现

### (8) Token 上下文压缩

每次对话前自动判定：消息数 > 32 且前 16 条不空了 → 触发压缩。
策略：
1. 把最早 N 条非空 user/assistant 抽取
2. 拼成一段 `# 早期对话摘要（N 条）` 插入 system 段尾
3. 用切片替代原 messages（前段 system + 后段 N 条；中间删除）

阈值固定在 MVP。可以调 `memory::compressor::maybe_compact(&mut s, 32, 16)`。

立即触发：`/compact`。

### (9) Sub-agent 委派

LLM 端可见 2 个工具：
- `spawn_agent({prompt, model?})` → 立刻返回 `task_id`；后台异步 spawn 任务
- `task_output({task_id, block?})` → 查询状态（pending / running / completed / failed）+ output

任务仓库在 `Arc<SubAgentRegistry>`（session 内共享）。

MVP 注意：当前 stub 把 prompt 回显进 output；下轮把 prompt 派给独立 LLM session。

### (10-12) 自我记忆进化（web 搜索版）⭐ Round 3

完整解决了「**AI 不单靠用户对话、也能从公网自我进化**」。

#### 数据布局

```
~/.fr_cli/memory/
├── topics.json                  # 用户关注的话题列表
├── long-term.md                 # 累积的长期记忆（注入到 system prompt）
└── evolution/                   # 每次进化的快照
    ├── 2026-07-10-evo-161410.md
    ├── 2026-07-10-evo-161500.md
    └── ...
```

#### 命令

```
/memory_topics                          # 列出
/memory_topics add <name>               # 加话题（"rust", "claude", "fr-cli"...）
/memory_topics rm <name>                # 删话题
/memory_evolve                          # 手动跑一次
/memory_evolve auto on [secs]           # 后台定时（默认 1800s = 30min）
/memory_evolve auto off                 # 停
```

每次 `/memory_evolve` 跑：
1. 读 `topics.json`，对每个 topic 拼 query（自动加年份后缀暗示"最近"）
2. 调 search backend（默认 DuckDuckGo HTML scraper；`BRAVE_API_KEY` 环境变量自动切 Brave API）
3. 抓每个 topic 的前 K 个 hit（title / url / snippet），组装 markdown 写到 `evolution/<timestamp>.md`
4. 精简版追加进 `long-term.md`
5. 更新 `topics.json` 的 `last_evolved_at`

#### 启动期自动注入

`long-term.md` 末尾 **8KB** 截断后注入到 system prompt 尾段，下次重启自动继承记忆。

LLM 还能直接通过两个工具维护：
- `memorize({content, source?})` —— 直接写一条事实到 long-term
- `recall({query, k?})` —— 按关键词扫 long-term + evolution/*.md 命中
- `web_search({query, k?})` —— 临时触发公网搜索（不一定要进 auto timer）

#### Backend 配置

```bash
# 默认无 key 用 DuckDuckGo HTML scraper
./target/release/fr
fr> /memory_evolve

# 用 Brave Search（更稳，免费 2000/月）
export BRAVE_API_KEY=BSA_xxxxxxxxxxxxxxxxx
./target/release/fr
```

#### 实战例子

```bash
$ fr --logo
fr> /memory_topics add rust
fr> /memory_topics add claude
fr> /memory_topics add fr-cli
fr> /memory_evolve
⏳ 进化中：3 个话题，backend = duckduckgo-html

· rust        (5 hits)
   1. Announcing Rust 1.96  https://blog.rust-lang.org/...
   2. ...
· fr-cli      (3 hits)
   1. OpenClaw: A 'Local-first AI Agent' ...  https://github.com/...
· claude      (4 hits)

✓ 进化完成  快照写入 ~/.fr_cli/memory/evolution/2026-07-10-evo-161410.md
            顺带 append 到 long-term.md
```

下一次重启后，LLM 已经"知道"今天最近一周 rust / claude / fr-cli 发生了什么。

### (13-14) Hooks 系统 ⭐ Round 4

跟 OpenClaw / Claude Code 完全一致的 4 类事件钩子系统，配合工具调用管线深度集成。

#### 4 个事件

| 事件 | 触发时机 | hook 能做什么 |
|---|---|---|
| `SessionStart` | REPL 启动一次 | 加载项目状态 / 写审计日志 / 注入 SSO token |
| `UserPromptSubmit` | 用户文本送进 LLM 前 | 阻止 / 改写 prompt / 追加 `additional_context` |
| `PreToolUse` | 工具调用前 | `exit 2` 阻止；stdout envelope 改 `tool_args` |
| `PostToolUse` | 工具返回后 | 写审计 / 累计指标 / 通知 webhook |

#### 配置文件 `~/.fr_cli/hooks.json`

```json
{
  "PreToolUse": [
    {
      "matcher": "shell",
      "hooks": [
        {
          "type": "shell",
          "command": "/Users/liangyj/.fr_cli/hooks/audit-pre-shell.sh",
          "timeout_ms": 1500
        }
      ]
    }
  ],
  "PostToolUse": [],
  "UserPromptSubmit": [],
  "SessionStart": []
}
```

- `matcher` 是工具名 regex（`shell|write_file`）；事件级 hook 用 `"*"`
- `type: "shell"` 跑 `sh -c <command>`；HookInput 通过 stdin JSON 喂入
- 阻止语义：**`exit 2` 或 stdout envelope `{"continue_": false}`**

#### stdout envelope（hook 改 LLM 行为的协议）

```bash
echo '{"additional_context":"用中文回答用户"}'
echo '{"modified_args":{"path":"/safe/file.txt"}}'
echo '{"modified_prompt":"在原 prompt 前加 [Classified]"}'
echo '{"continue_":false,"reason":"rm 命令禁止"}'   # exit code 即可，无需这条
exit 2                                                # 直接阻止
```

#### 命令

```bash
/hooks                  # 列出当前 hooks 配置
/hooks events           # 4 类事件详解
/hooks init             # 生成样板 hooks.json
/hooks reload           # hot-reload（无需重启 REPL）
/hooks path             # 配置文件路径
```

#### 实战

`~/.fr_cli/hooks.json` 默认含：
- `PreToolUse shell` —— 审计前置
- `PostToolUse *` —— 跑完工具后通知
- `UserPromptSubmit *` —— 把 `[Hook 提醒]` 拼到 user 消息尾
- `SessionStart *` —— REPL 启动时打印标语

当用户在 REPL 输入一条 prompt，会看到：
```
  [session_start hook]   [SessionStart] FrClaw 已就绪

─────────────────────────────────────────
  [UserPromptSubmit hook]   [UserPromptSubmit] 收到 prompt
  [UserPromptSubmit hook → additional_context 已注入 user 消息尾部]

[info] (FR_NO_LLM=1，跳过真实 LLM 调用；hook 链路已跑)
```

`/hooks show` 打印当前配置：
```
[PreToolUse]  (2 条)
  1. matcher=`shell` (1 hooks)
     - shell: echo "  [PreToolUse:shell] 你正在跑 shell" >&2  (timeout 1500ms)
[SessionStart]  (1 条)
  1. matcher=`*` (1 hooks)
     - shell: echo "  [SessionStart] FrClaw 已就绪" >&2  (timeout 1500ms)
```

#### 安全模型

- 每个 hook 独立超时（`timeout_ms`，最低 50ms）
- hook 用 `sh -c` 执行（不会拿到完整 shell 上下文）
- hook stderr 默认不返回（除非 `suppress_stderr=false`）
- hook 默认 `continue_=true` —— 你必须显式 `exit 2` 才会被认作阻止
- 关键 hook 建议放独立脚本文件（`chmod 700 ~/.fr_cli/hooks/*.sh`）

### (15) Skills 系统 ⭐ Round 5

跟 OpenClaw 5,700+ Skills 生态兼容：用户用 `.md` 文件定义工作流模板，AI 命中 trigger 后自动加载。

#### 文件位置

```
~/.fr_cli/skills/                          # 用户自定义（hot-reload）
└── my-skill/SKILL.md

assets/skills/                             # builtin 随包发布
├── web-research/SKILL.md
└── project-overview/SKILL.md
```

#### SKILL.md schema

```markdown
---
name: web-research
description: 用 web 搜索 + 摘要 + 长期记忆，研究一个主题
triggers:
  - "研究"
  - "调研"
  - "web search"
  - "@research"
allowed-tools:
  - web_search
  - memorize
  - recall
max-steps: 8
---

# Web Research Skill

## Step 1 — 拆解 query
如果用户问的是模糊的「最近 xxx 怎么样」，先用 `recall({query: "..."})` ...

## Step 2 — 多个角度搜索
对核心主题，跑 2-3 次 `web_search({query, k: 3})` ...

## Step 3 — 整合 + 摘要
把命中按可信度排 ...
```

字段：
- `name` —— skill 名（默认用父目录名）
- `description` —— 一句话描述（/skill list 时显示）
- `triggers` —— 触发词列表（substring / regex / `@alias` 三种）
- `allowed-tools` —— 允许 LLM 调的工具白名单（空 = 全开）
- `max-steps` —— agent 最大步数

#### trigger 匹配

| 形态 | 例子 | 匹配方式 |
|---|---|---|
| substring | `"研究"` | 用户输入 `case-insensitive contains "研究"` |
| regex | `"weather\|forecast"` | 编译为 regex 匹配 |
| alias | `"@research"` | 用户输入任意 token 形如 `@research` |

#### 自动注入

当用户输入命中某个 skill 的 trigger 时，skill body 自动拼到 user 消息尾（这一轮生效）：

```
fr> 帮我调研一下最近 rust 1.96 的特性
─────────────────────────────────────────
  [skills] 触发命中 1 条: web-research
[info] (FR_NO_LLM=1，跳过真实 LLM 调用；hook 链路已跑)
```

#### 命令

```bash
/skill list             # 列出所有 skill
/skill show <name>      # 打印 SKILL.md 全文
/skill dir              # 输出用户 skill 目录路径
/skill reload           # hot-reload 用户目录
/skill path             # 同时显示 user / builtin 路径
```

#### 写自己的 skill

```bash
mkdir -p ~/.fr_cli/skills/code-review
$EDITOR ~/.fr_cli/skills/code-review/SKILL.md
# frontmatter 写 triggers: ["review", "代码审查", "@cr"]
# body 写流程步骤

fr> /skill reload
fr> 帮我 review 一下 src/auth.rs
# → 自动加载 code-review skill
```

#### 安全模型

- skill body 在 user 消息里 —— 不会让 AI「以为它是 system 级」
- 工具白名单 `allowed-tools` 仅展示给 LLM 选择倾向，**不强制拦截**（要拦截得靠 Hooks）
- 想完全隔离敏感操作，把 hooks 的 `PreToolUse` 接上

### (16) MCP Streamable HTTP client ⭐ Round 6

FrClaw 现在能直接接 [Model Context Protocol](https://modelcontextprotocol.io) 生态——Anthropic 主导的「LLM 工具互操作标准」，已有 5,700+ 公开 MCP server（filesystem / github / postgres / playwright / 各种内部工具）。本实现是**自写**的 Streamable HTTP transport，不依赖第三方 MCP crate——协议层 + 客户端 + 配置 + 注册表 + 路由 + 命令全在 `src/mcp/` 下，约 600 行。

#### 协议覆盖（spec 2025-06-18）

- 单端点 `POST /mcp` + JSON-RPC 2.0
- `MCP-Protocol-Version: 2025-06-18` header
- `Mcp-Session-Id` header（session 复用）
- 已实现：`initialize` + `notifications/initialized` + `tools/list` + `tools/call`
- 响应：仅 `application/json`（流式 `text/event-stream` 留作下轮）

#### 配置 `~/.fr_cli/mcp_servers.json`

```json
{
  "servers": [
    {
      "name": "filesystem",
      "url": "http://127.0.0.1:8123",
      "auto_connect": true,
      "enabled": true,
      "headers": { "Authorization": "Bearer xxx" },
      "timeout_ms": 30000
    },
    {
      "name": "github",
      "url": "https://mcp.example.com/github",
      "headers": { "Authorization": "Bearer ghp_xxx" }
    }
  ]
}
```

字段：
- `name` —— 内部 alias，工具命名空间用 `mcp__<name>__<tool>`
- `url` —— Streamable HTTP 端点 base URL（POST `{url}/mcp`）
- `auto_connect` —— 启动时自动 connect 并缓存 tools 列表（默认 true）
- `enabled` —— `false` 时跳过
- `headers` —— 透传 HTTP header（鉴权 token）
- `env` —— 预留（给本地 stdio server 用，下轮）
- `timeout_ms` —— 单次请求超时

#### 命令

```bash
/mcp list                       # 列 server 状态
/mcp add <name> <url>           # 加 server
/mcp remove <name>              # 删 server
/mcp reconnect <name>           # 重连
/mcp tools                      # 列所有 server 暴露的 tools
/mcp call <server> <tool> [args-json]   # 手动调
/mcp config                     # 配置文件路径
```

#### LLM 端使用

MCP server tools 自动并入 LLM 工具列表（命名为 `mcp__<server>__<tool>`），跟本地 tool 走同一 dispatcher：

```
fr> 这两个 JSON 文件 diff 一下
─────────────────────────────────────────
  [mcp__filesystem__read_file]  ←  LLM 决定调
  args: { "path": "/Users/.../models.yaml" }
  [mcp__filesystem__read_file]
  args: { "path": "/Users/.../settings.json" }
```

#### 容错

- server 连不上 → 标记 `Failed`，不影响其它 server；`/mcp reconnect <name>` 重建
- session id 永久化：connect 一次后 server 给的 `Mcp-Session-Id` 缓存复用
- 单 server 失败不会阻断启动；用户从 `/mcp list` 看得到
- `tokio::sync::RwLock` + 直构 state（避免 `from_config` 时的 `blocking_write` deadlock）

#### 端到端测试（已实跑通）

用一个 mock MCP server（`/tmp/mock_mcp.py`，~80 行 stdlib http server，实现 initialize / tools/list / tools/call）：

```bash
$ python3 /tmp/mock_mcp.py 8123 &
$ fr --logo
fr> /mcp add mock http://127.0.0.1:8123
✓ 已添加 `mock` -> http://127.0.0.1:8123

# 第二次启动
fr> /mcp list
  • mock  connected  http://127.0.0.1:8123
    tools (3):
      - mcp__mock__echo       echo back the input
      - mcp__mock__add        sum two numbers
      - mcp__mock__list_files list files in a directory (mock — ...)

fr> /mcp call mock add {"a":10,"b":32}
{
  "content": "42"
}
```

### (17) RAG 个人知识库 ⭐ Round 7

`memorize` 只能记单条事实，recall 是关键词扫；Round 7 起 FrClaw 有了真正的 **vector 知识库**——用 sqlite 存 chunks + embedding，LLM 可调 4 个工具，REPL 可跑 8 个子命令。

#### 存储

- **单 sqlite 文件**：`~/.fr_cli/rag.db`（WAL 模式）
- **chunks 表**（id, source, chunk_index, content, embedding BLOB, dim, origin, metadata, created_at）
- **sources 表**（name, count, last_updated）

不引 `sqlite-vec` 扩展——加载到内存做 naive cosine，< 10k chunks 时是微秒级。**避免扩展加载的跨平台坑**。

#### Embedding

- **第一路**：调 provider `/embeddings` 端点（OpenAI / 智谱 / DeepSeek 都支持）
- **第二路 fallback**：纯本地 **256-dim hashing TF**（tokenize → SHA-256 → signed count → L2 归一化）
  - 零网络、零依赖
  - 中文 / 英文 / 混合都能 work
  - 跟 `fastembed-mini` 离线模式思路一致
- 没配置 provider embedding 时直接用 hash，**开箱即用**

#### Chunking

- 段落（`\n\n`）切，过长按句子（`.!?。！？\n`）切，仍过长按窗口切
- ~500 字符 / chunk，50 字符 overlap

#### 工具（LLM 可见）

| 工具 | 用途 |
|---|---|
| `rag_add({source, content, metadata?})` | 入库一段文本 |
| `rag_query({query, k?})` | top-k 检索（默认 k=5） |
| `rag_list()` | 列所有 source + chunk 数 |
| `rag_remove({source})` | 按 source 名删除 |

`memorize` 工具**自动双写一份到 RAG**（`memory:<source>` 命名空间），让 LLM 沉淀的事实直接进知识库。

#### 命令

```bash
/rag list                       # 列出 source
/rag add <source> <text>        # 手动入库
/rag query <text>               # 检索 top-5
/rag show <source>              # 看某 source 全部 chunk
/rag remove <source>            # 删除 source
/rag import <path> [<name>]     # 导入文件 / 目录（.md/.txt/.json/.rst）
/rag auto on|off                # auto-recall 开关
/rag status                     # 总览
```

#### Auto-recall（默认开）

`/rag auto on`（默认开，`FR_RAG_AUTO=0` 可关）后，**用户消息 > 4 字**时会自动 query RAG，把 top-2 chunk 拼成 `[RAG 召回]` 段注入 user 消息尾：

```
fr> rust 的所有权系统是怎么设计的
  [UserPromptSubmit hook]  收到 prompt
  [rag] auto-recall 命中 2 条
[user] [zhipu] rust 的所有权系统是怎么设计的

[RAG 召回]
- [rust-intro #0] (score=0.412) Rust 是一门系统级语言…
- [notes.md #1] (score=0.156) Cargo 是 Rust 的包管理工具。
```

#### 实战

```bash
$ fr --logo

fr> /rag import ~/Documents/notes/
  ✓ project-overview.md → 3 chunks
  ✓ q3-review.md → 5 chunks
目录导入完成：8 chunks

fr> /rag status
RAG status:
  db:        /Users/liangyj/.fr_cli/rag.db
  size:      36.00 KB
  chunks:    8
  sources:   2
  embedder:  hash-baseline (256-dim, 零网络)
  auto:      on

fr> 上季度复盘提到哪些关键指标
  [rag] auto-recall 命中 2 条
[user] [zhipu] 上季度复盘提到哪些关键指标
[RAG 召回]
- [q3-review.md #1] (score=0.318) 核心指标：GMV 同比 +18%…
- [q3-review.md #3] (score=0.201) 留存：DAU/MAU 28%…
```

#### 容错

- 写入失败 → 单 chunk 跳过，不影响后续
- 检索 OOM 风险：< 10k chunks × 256 floats = ~1MB 内存，零压力
- `open_in_memory` fallback：db 路径无法写时退到 `:memory:`，不阻断 REPL

### (18) Git worktree + 原子 multi_edit ⭐ Round 8

让 AI 改代码更安全——**worktree 隔离 + 原子多文件编辑**。

#### Git worktree 工具（LLM 可见 4 个）

| 工具 | 用途 |
|---|---|
| `worktree_create({path, branch?, from?})` | 新建 worktree（独立分支 / 目录） |
| `worktree_list()` | 列所有 worktree（主 + 链接） |
| `worktree_remove({path, force?})` | 删除 worktree |
| `worktree_status({path?})` | 看 branch + clean/dirty + dirty files |

启动期从 cwd 向上探测 `.git`，缓存在 `WorktreeContext`。不在 git 仓库里 → 工具返回 `{"error": "..."}`。

底层走 `git worktree` / `git rev-parse` / `git status --porcelain` 子命令，**不引 libgit2**。

#### multi_edit 工具（LLM 可见）

AI 提议 N 处编辑，一次原子应用；任一失败，全部回滚。

```json
{
  "edits": [
    {"path": "/abs/a.rs", "old_text": "fn old()", "new_text": "fn new()"},
    {"path": "/abs/b.rs", "old_text": "x = 1", "new_text": "x = 2"}
  ],
  "create_if_missing": false
}
```

**保证**：
- `old_text` 必须**精确出现 1 次**（0 次 / N>1 次都报错）
- 同一文件多次 edit 顺序应用
- 写文件用 atomic write（写 `.tmp` → rename），减少中途崩溃风险
- **任何失败 → 还原所有已 snapshot 的文件**
- 成功不留 backup；失败留 `.bak.fr_multi_edit` 供人工查

#### 命令

```bash
/worktree list                       # 列 worktree
/worktree create <path> [<branch>]   # 新建
/worktree remove <path> [--force]    # 删除
/worktree status [path]              # 看状态
/worktree root                       # 打印 git root
/worktree refresh                    # 重新探测（cwd 变后用）

/multi_edit <inline-json>            # 内联 JSON
/multi_edit @/path/to/edits.json     # 从文件读
/multi_edit help                     # 用法
```

#### 实战

```bash
# 在 git 仓库里
$ fr --logo

fr> /worktree list
Git worktrees（1 个，root: /Users/me/proj）：
  ★ /Users/me/proj                 master

# 让 AI 在 feature-x 分支干活
fr> 在 wt-feature 上拉一个新分支写 feature
  [tool: worktree_create] { path: "wt-feature", branch: "feature-x" }
  [tool: worktree_status] { path: "wt-feature" }   # clean ✓

# 改完后原子应用 2 个文件
fr> 帮我把 fn alpha() 改成返回 i32，并且 b.rs 里把 y 改成 42
  [tool: multi_edit] {
    "edits": [
      { "path": "a.rs", "old_text": "fn alpha() {}", "new_text": "fn alpha() -> i32 { 0 }" },
      { "path": "b.rs", "old_text": "let y = 2;",  "new_text": "let y = 42;" }
    ]
  }
[info] ✓ 应用 2 处编辑，2 个文件
  • a.rs (1 处, 27→37 字节)
  • b.rs (1 处, 22→23 字节)

# 如果第二个 old_text 找不到，a.rs 也会自动回滚
fr> /multi_edit {"edits":[
  {"path":"a.rs","old_text":"hello","new_text":"HELLO"},
  {"path":"b.rs","old_text":"gamma","new_text":"WORLD"}
]}
[error] ✗ `b.rs` 找不到 `old_text`（5 字符）
  ↩ 已回滚 1 个文件:
    - a.rs
  (pre-edit 备份：<file>.bak.fr_multi_edit)
```

### (19) Sandbox 隔离 ⭐ Round 9

`shell` / `read_file` / `write_file` / `list_dir` 工具在执行前会过沙箱检查——**默认开启**。两道护栏：

#### 1. 路径白名单

- **read_allow**（默认）：cwd + `~/.fr_cli/` + `/tmp/` + `/private/tmp/` + `/Users`
- **write_allow**（默认）：cwd + `~/.fr_cli/` + `/tmp/`
- 越界 → 工具返回 `{"error": "...", "blocked_by": "sandbox"}`，文件不会被碰

#### 2. shell 命令黑名单（substring 匹配）

内置高危命令：

| 类别 | 模式 |
|---|---|
| 致命删 | `rm -rf /`, `rm -rf /*`, `rm -rf $HOME`, `rm -rf ~` |
| Fork bomb | `:(){ :|:& };:` / `:(){:\|:&};:` 等变体 |
| 磁盘 | `mkfs`, `dd if=`, `fdisk` |
| 系统 | `shutdown`, `reboot`, `halt`, `poweroff`, `init 0/6` |
| 写到原始设备 | `> /dev/sd*`, `> /dev/nvme*`, `> /dev/disk*` |
| 危险 chmod | `chmod -R 777 /`, `chown -R` |
| 远程下载 + 执行 | ` \| sh`, ` \| bash`, ` \| zsh`, `$(curl`, `$(wget` |

#### 3. 资源限制

- **timeout**：30s（policy 可改）
- **max_stdout**：1MB（截断）
- **network**：默认 allow，可切 deny

#### 4. macOS 增强（可选）

macOS 上 strategy 设 `use_macos_sandbox_exec = true` 时，shell 调用会包一层 `sandbox-exec -f scheme.sb`，让 macOS 内核做 syscall 级隔离（`sandbox-exec` 是 macOS 自带，不引第三方）：

```scheme
(version 1)
(deny default)
(allow process-exec)              ; sh -c 跑得起来
(allow process-fork)              ; 子进程
(allow sysctl-read)
(allow mach-lookup)
(allow file-read* (subpath "/"))
(allow file-write* (subpath "/Users/.../cwd"))
(allow file-write* (subpath "/Users/me/.fr_cli"))
(allow network*)
```

#### 持久化

`~/.fr_cli/sandbox.json`（策略改动 `/sandbox save` 后写入）：

```json
{
  "enabled": true,
  "read_allow": ["${cwd}", "${home}/.fr_cli", "/tmp"],
  "write_allow": ["${cwd}", "${home}/.fr_cli"],
  "shell_deny": ["rm -rf /", ":(){ :|:& };:", "..."],
  "network": "allow",
  "max_stdout_bytes": 1048576,
  "timeout_ms": 30000,
  "use_macos_sandbox_exec": true
}
```

`${cwd}` / `${home}` 启动时展开成实际路径。

#### 命令

```bash
/sandbox status                   # 打印当前策略
/sandbox on|off                   # 总开关
/sandbox save                     # 写入 sandbox.json
/sandbox reload                   # 从 sandbox.json 重载
/sandbox test <shell-cmd>         # 测一个命令是否允许
/sandbox allow <read|write> <path>   # 运行时加白
/sandbox deny <pattern>           # 运行时加黑
```

#### 实战

```bash
$ fr --logo
fr> /sandbox test "rm -rf /"
[error] ✗ 阻止: shell 命令命中沙箱黑名单 `rm -rf /`

fr> /sandbox test "ls -la"
[info] ✓ 允许: "ls -la"

fr> /sandbox test ":(){:|:&};:"
[error] ✗ 阻止: shell 命令命中沙箱黑名单 `:(){:|:&};:`

fr> /sandbox test "echo hello | sh"
[error] ✗ 阻止: shell 命令命中沙箱黑名单 ` | sh`

fr> /sandbox test "dd if=/dev/zero of=/dev/sda"
[error] ✗ 阻止: shell 命令命中沙箱黑名单 `dd if=`

# 临时关沙箱（不推荐）
fr> /sandbox off
fr> /sandbox test "rm -rf /"
[info] ✓ 允许: "rm -rf /"
```

工具级返回（LLM 收到）：

```json
{
  "cmd": "rm -rf /",
  "error": "shell 命令命中沙箱黑名单 `rm -rf /`",
  "blocked_by": "sandbox",
  "ok": false
}
```

### (20) Streaming Markdown 增量渲染 ⭐ Round 10

之前 LLM 流式响应时直接 `print!` 原始字符——代码块、标题、列表都没颜色，输出糊在一起。Round 10 起，每次 LLM delta 来时过 **行级状态机**：

- **行级 flush**：token 累加到 `MarkdownStream` 内部 buffer；遇到 `\n` 立刻渲染该行（含 ANSI 颜色）并 stdout
- **强制 flush 阈值**：单行超过 200 字符无 `\n` 强制 flush（防 LLM 一行超长卡住）
- **`finish()` 收尾**：最后一次 step 结束时 flush 剩余内容

复用了已有的 `ui::markdown::render()` 整段渲染逻辑，但用 `last_rendered.len()` 做 diff，只 print 新增的字节（不重复、不漏行）。

#### 支持的语法

| 语法 | 渲染效果 |
|---|---|
| `# / ## / ###` | 粗体 + 青色（ANSI 1;36） |
| ```` ``` ```` 围栏 | 整块 dim 色（灰），保留原内容 |
| 行内 `` `code` `` | dim 反引号 |
| `**bold**` | ANSI bold |
| `*italic*` | ANSI italic |
| `- / * / +` 列表 | bullet 不变色，body 走 inline |
| `1. 2. 3.` 列表 | 数字黄色 + body inline |
| `> ` 引用 | 缩进 + dim |

`NO_COLOR=1` 时全部用纯文本，不加 ANSI 码。

#### 集成点

`agent::loop_runner::run_agent_step_loop` 的 `LlmReply` 回调里把 `content` 走 `MarkdownStream::add(&content)`，最后 `Final` / 流结束 `finish()`。其它路径（tool 预览、提示）不动。

#### 实战对比

之前流式输出长这样：
```
fn main() { println!("hi"); } 这是普通段落
```

现在（带颜色）：
```
fn main() { println!("hi"); }                ← 灰
这是普通段落                                  ← 白
```

`# 标题` → 青色加粗 + 段落白，`- list` → 黄色 bullet + 段落白。

### (21) Web 控制台 ⭐ Round 11

让 FrClaw 像 ZeroClaw 一样有 Web UI。`./fr web` 起 HTTP server，默认 `127.0.0.1:8765`。

#### 启动

```bash
$ ./fr web
  ╔══════════════════════════════════════════════╗
  ║         FrClaw Web 控制台 (Round 11)      ║
  ╚══════════════════════════════════════════════╝

  监听地址:  http://127.0.0.1:8765
  Token:     e33999cb1336afa23705467032932214c5ce7c4d2daa15a4665473eeb1476f84
             (已存到 /Users/liangyj/.fr_cli/web_token)

  打开浏览器:
     http://127.0.0.1:8765/?token=e33999cb...
```

选项：

| Flag | 默认 | 用途 |
|---|---|---|
| `--port <N>` | 8765 | 监听端口 |
| `--host <H>` | 127.0.0.1 | 监听地址（外网 0.0.0.0 不推荐除非配合鉴权） |
| `--no-auth` | off | 关闭 Bearer Token 鉴权（不推荐） |
| `--token <T>` | 自动生成 / 持久化 | 用已存在的 token，不重新生成 |

#### 路由

| 路径 | 方法 | 用途 |
|---|---|---|
| `/` | GET | 单页 chat UI（HTML + 原生 JS + EventSource） |
| `/static/style.css` | GET | 暗色主题 CSS |
| `/api/status` | GET | 状态 JSON（provider / cwd / sandbox / rag / mcp / skills） |
| `/api/skills` | GET | skills 列表 |
| `/api/sandbox` | GET | sandbox 策略 |
| `/api/mcp` | GET | MCP servers + tools |
| `/api/rag` | GET | RAG stats + sources |
| `/api/chat` | POST | SSE 流式 chat（`{ "message": "..." }`） |
| `/api/command` | POST | 跑一个 REPL 命令，返回 JSON |

#### 鉴权

- **默认开**：`Authorization: Bearer <token>`
- Token 启动时随机生成，存到 `~/.fr_cli/web_token`（权限 0600）
- 没带 token / token 不对 → 端点返回 `{"error":"unauthorized"}`
- HTML 页面的 `?token=xxx` 会被存到 localStorage，后续 fetch 自动加 header

#### SSE 事件类型（`/api/chat`）

每个 event 形如：`event: <name>\ndata: <json>\n\n`

| 事件 | payload | 用途 |
|---|---|---|
| `delta` | `{"type":"delta","content":"..."}` | LLM 流式输出片段 |
| `tool_calls` | `{"type":"tool_calls","calls":[...]}` | LLM 决定调工具 |
| `tool_result` | `{"type":"tool_result","name":"...","preview":"..."}` | 工具结果预览 |
| `error` | `{"type":"error","message":"..."}` | 错误 |
| `done` | `{"type":"done"}` | 流结束 |

#### Chat UI

- 暗色主题（`#1a1a1a` 背景 / 灰气泡）
- Enter 发送，Shift+Enter 换行
- 工具调用折叠在灰色短行
- 顶栏显示实时状态（provider · cwd · sandbox on · rag chunks · mcp N/M · skills N）

#### 实战

```bash
# 启动
$ ./fr web

# 用 curl 测
TOKEN=$(awk '/Token:/{print $2}' /tmp/fr-web.log)
$ curl -sS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8765/api/status
{"provider":"zhipu","cwd":"/...","sandbox":{"enabled":true},"rag":{"chunks":6},...}

# 流式 chat（带 LLM key 才能真跑）
$ curl -N -X POST -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"message":"用 rust 写个 hello"}' \
    http://127.0.0.1:8765/api/chat

event: delta
data: {"type":"delta","content":"# Rust Hello\n\n```rust\nfn main() { ...\n"}

event: tool_calls
data: {"type":"tool_calls","calls":[]}

event: done
data: {"type":"done"}
```

### (22) Hermes 后台任务引擎 ⭐ Round 12

OpenClaw 风格的后台任务：持久队列 + cron 调度 + 审核流程。文件落到 `~/.fr_cli/tasks.db`（sqlite）。

#### 任务类型

| kind | 说明 |
|---|---|
| `shell` | `sh -c <args>` 跑命令 |
| `prompt` | 触发 LLM（worker 模式无 LLM 上下文，预留） |
| `rag_query` | 跑 RAG 检索（预留） |
| `web_search` | 跑公网搜索（预留） |

#### 状态机

```
pending ──approve──> approved ──tick──> running ──┬─> completed
   │                                              └─> failed
   └─reject──> rejected
```

`approved` + `next_run_at <= now` → 后台 tick 抓出来执行；`shell` 任务同步跑、`prompt`/`rag_query`/`web_search` 留 placeholder。

#### Cron 5-field 表达式

```
分 时 日 月 周
* * * * *        # 每分钟
*/15 * * * *     # 每 15 分钟
0 9-17 * * 1-5   # 工作时间整点
0 0 * * 0        # 每周日 0 点
```

字段语法支持 `*` / `5` / `1,3,5` / `1-5` / `*/5` / `1-10/2`。

#### 后台 tick

启动后 tokio 后台 task 每 30s 扫一次 `due_tasks`（`approved` + `next_run_at <= now`），spawn 出来跑。

#### 命令

```bash
/tasks list                # 列所有 task
/tasks show <id>           # 看详情 + 历史 runs
/tasks add <name> <kind> [args] [--cron EXPR] [--auto]
                           # kind: shell / prompt / rag_query / web_search
                           # --auto: 自动审批（cron 任务默认）
/tasks approve <id>        # 审批 pending
/tasks reject <id>         # 拒绝 pending
/tasks run <id>            # 手动跑一次（无论 cron / 状态）
/tasks delete <id>         # 删除

/cron list                 # 列出所有带 cron 的 task
/cron validate <EXPR>      # 校验 + 算下一次触发
```

#### 实战

```bash
$ fr --logo

fr> /cron validate "*/15 * * * *"
✓ 合法：*/15 * * * *
  下一次触发: 2026-07-11 12:45:00 UTC

fr> /cron validate "0 9-17 * * 1-5"
✓ 合法：0 9-17 * * 1-5
  下一次触发: 2026-07-13 09:00:00 UTC

fr> /tasks add hello shell "echo hi from hermes"
✓ 创建 task #4 (hello, shell)

fr> /tasks add daily-cleanup shell "echo daily" --cron "0 0 * * *"
✓ 创建 task #5 (daily-cleanup, shell, cron=`0 0 * * *`)

fr> /tasks list
Hermes tasks（2 个）：
  #5   daily-cleanup            [  pending] cron=0 0 * * *      ran=0   last=—
  #4   hello                    [  pending] cron=—              ran=0   last=—

fr> /tasks approve 4
✓ task #4 已审批

fr> /tasks run 4
(hermes 手动触发 #4 ... )
✓ 完成

fr> /tasks show 4
  id:        4
  name:      hello
  kind:      shell
  status:    completed
  run_count: 1
  runs (1):
    [completed] 2026-07-11 12:40:17 (0s)
```

### (23) SOUL.md 持久身份 ⭐ Round 13

仿 OpenClaw 的 SOUL.md 概念 —— 用一个或多个 markdown 文件定义 AI 的 persona / voice / tone / 价值观 / 偏好 / 准则。启动期读入，注入到 system prompt 尾段（在 long-term.md 之后）。

#### 多源（按优先级合并）

1. `~/.fr_cli/soul.md`（全局）
2. `<cwd>/SOUL.md`（项目级）
3. `<cwd>/AGENTS.md`（兼容 OpenClaw / Claude Code 习惯）

合并算法：按 `## 段` 切分；高优先级 source 的同名段**覆盖**低优先级；无标题 intro 全部保留。

#### 注入 system prompt

启动时把合并内容拼到 system prompt 末尾：

```text
# SOUL（持久身份 / 价值观 / 准则）

## persona
You are a calm, deliberate coding assistant. ...

## voice
中文 / 英文混排；技术术语保留英文。 ...

## values
- 先想清楚再动手
- 一次性原子
- 解释 trade-off
```

#### 工具（LLM 可见 2 个）

| 工具 | 用途 |
|---|---|
| `read_soul()` | 读当前 SOUL 内容（自检身份） |
| `append_soul({text})` | 追加一条（不修改原内容） |

`append_soul` 只追加、不修改 —— 让 LLM 学到新偏好时能记录，但不破坏原 persona。

#### 命令

```bash
/soul show                  # 打印当前 SOUL（合并后）
/soul path                  # 打印 SOUL 文件路径
/soul edit                  # $EDITOR 打开全局 SOUL.md
/soul append <text...>      # 快速追加
/soul init                  # 第一次初始化（写默认模板）
/soul reload                # 从磁盘重载（忽略缓存）
```

#### 实战

```bash
$ fr --logo

fr> /soul init
✓ 初始化 SOUL: /Users/me/.fr_cli/soul.md

fr> /soul show
SOUL sources（1 个）：
  [global] /Users/me/.fr_cli/soul.md

──── 合并后内容 ────
  # SOUL — 持久身份
  ## persona
  You are a calm, deliberate assistant.
  ## voice
  中英混排，简洁直接。
────────────────────

fr> /soul append "## new rule
Always check cwd before shell commands"
[info] ✓ 已追加 47 字符到 /Users/me/.fr_cli/soul.md

fr> /soul reload
[info] ✓ SOUL 重载完成（1 个 source，178 字符）

# 之后每次启动都会自动把 SOUL 注入 system prompt；
# AI 看到 persona / values 后会按其行为。
```

## 内置命令一览（40 个）

```
会话管理    /new /save /load /list_sessions /see
模型配置    /model /providers /key /lang /limit
agent 行为  /autonomous /mode /memory /memory_topics /memory_evolve /compact /hooks /skill /mcp /rag /worktree /multi_edit /sandbox /tasks /cron /soul
工作目录    /dir /read /write /shell
Web        /web (subcommand)
LLM 调用    <直接输入文本>
其他        /banner /version /clear /doctor /help /exit
```

工具调用授权提示（默认 autonomous=false）：
```
工具 `shell` 请求授权  args: {"cmd":"rm /tmp/x"}
授权方式 (y=这一次 / a=always / f=full-auto / n=no):
```
- `y` —— 仅这一次同意
- `a` —— 这一类工具后续不再问
- `f` —— 进入 full-auto（session 内不再问）
- `n` —— 拒绝，agent 把"用户拒绝"作为 tool 响应回灌给 LLM

`read_file` / `list_dir` 是 read-only 工具，自动跳过授权。

## 模块组织（Round 2 后）

```
src/
├── main.rs                  ── 入口
├── lib.rs                   ── 库根
├── error.rs                 ── Error + Result
├── cli/
│   ├── args.rs              ── clap 参数
│   └── bootstrap.rs         ── 加载配置 + 项目记忆 + AppContext
├── repl/
│   ├── runner.rs            ── REPL 主循环 (rustyline)
│   ├── command.rs           ── / 命令路由器 + 文本输入派发 → agent
│   ├── commands.rs          ── 28 个命令实现
│   └── context.rs           ── AppContext（共享 state + agent 状态）
├── config/
│   ├── paths.rs             ── ~/.fr_cli/ 路径
│   ├── models.rs            ── models.yaml 加载
│   ├── keys.rs              ── env / keys.json 双层 key 解析
│   └── settings.rs          ── settings.json
├── llm/
│   ├── message.rs           ── 统一 message model + tool calls
│   ├── provider.rs          ── LlmProvider trait
│   ├── openai_compat.rs     ── OpenAI 兼容 (SSE 流式 + 工具调用增量解析)
│   ├── registry.rs          ── Provider 工厂 + 降级链
│   └── prompts.rs           ── 中英文 system prompt
├── agent/
│   ├── mod.rs               ── 公开入口
│   ├── loop_runner.rs       ── **ReAct step loop（核心）**
│   └── prompts.rs           ── 5 种 ThinkingMode
├── session/
│   ├── chat.rs              ── ChatSession
│   └── store.rs             ── 持久化
├── tools/
│   ├── mod.rs               ── builtin tools (read/write/list/shell)
│   ├── registry.rs          ── LLM 可见 ToolDefinition + dispatch
│   ├── permission.rs        ── **4 阶授权 (Y/A/F/N)**
│   ├── plan.rs              ── **Plan mode (enter/exit_plan_mode tool)**
│   ├── subagent.rs          ── **Sub-agent 委派 (spawn_agent / task_output)**
│   ├── web_search.rs        ── **公网搜索（DuckDuckGo / Brave）**
│   └── memorize.rs          ── **memorize / recall（长期记忆的写入与召回）**
├── memory/
│   ├── project.rs           ── **项目记忆自动加载**
│   ├── compressor.rs        ── **Token 上下文压缩**
│   └── evolution.rs         ── **自我记忆进化（web 搜索引擎 + auto timer + 长期记忆索引）**
└── ui/
    ├── colors.rs            ── owo-colors (NO_COLOR)
    ├── banner.rs            ── 启动 banner
    └── markdown.rs          ── 极简 markdown 渲染
```

## 快速开始

```bash
cargo build --release

# 启动 REPL
./target/release/fr --logo

# 设置 API key（写入 ~/.fr_cli/keys.json，权限 0600）
fr> /key zhipu sk-xxxxxxxxxxxxxxx

# 或走 env
ZHIPU_API_KEY=sk-xxx ./target/release/fr

# 一次性提问（one-shot）
./target/release/fr -p "把这句话翻译成英文：FrClaw"

# 切到本地 Ollama
fr> /model ollama
fr> 你好

# 用 Plan mode 跑多步任务
fr> /mode plan
fr> 帮我研究下当前目录下所有 .rs 文件并生成一份 module 概览
# → AI 会先 enter_plan_mode 给出步骤，你审 y 后才执行
```

## 配置文件 `~/.fr_cli/models.yaml`

```yaml
providers:
  zhipu:
    name: 智谱 GLM
    model: glm-4-flash
    protocol: openai
    base_url: https://open.bigmodel.cn/api/paas/v4
    api_key_env: ZHIPU_API_KEY
    is_default: true
  deepseek:
    name: DeepSeek
    model: deepseek-chat
    protocol: openai
    base_url: https://api.deepseek.com/v1
    is_backup: true
settings:
  default_provider: zhipu
  backup_provider: deepseek
  history_window: 5
  lang: zh
```

## 路线图

**已交付**：Round 1 (MVP) + Round 2 (9 个高级特性) + Round 3 (自我记忆进化) + Round 4 (Hooks) + Round 5 (Skills 系统) + Round 6 (MCP Streamable HTTP client) + Round 7 (RAG 个人知识库) + Round 8 (Worktree + 原子 multi_edit) + Round 9 (Sandbox 隔离) + Round 10 (Streaming Markdown 增量渲染) + Round 11 (Web 控制台) + Round 12 (Hermes 后台任务引擎) + Round 13 (SOUL.md 持久身份) + Round 14 (Heartbeat 主动唤醒) + Round 15 (4 项改进：MCP Resources/Prompts / RAG hybrid / PPTX / TTS) + **Round 16 (6 项集成：飞书/钉钉/企微/Webhook/Timeline HTML/Voice STT)**

**Round 16 新增**：

| 模块 | 文件 | 干啥 |
|---|---|---|
| `channels/mod.rs` | `Channel` trait + `OutboundMessage` + `SendReceipt` | 4 个 channel 共用抽象 |
| `channels/config.rs` | `ChannelsFile` + `ChannelConfig` | `~/.fr_cli/channels.json` 持久化 |
| `channels/lark.rs` | 飞书群机器人 | text / post 富文本，加签 HmacSHA256 |
| `channels/dingtalk.rs` | 钉钉群机器人 | text / markdown，加签 HmacSHA256 + URL 编码 |
| `channels/wecom.rs` | 企业微信群机器人 | text / markdown（自动截断 4096 字节） |
| `channels/webhook.rs` | 通用 webhook | POST JSON 到任意 URL |
| `channels/registry.rs` | `ChannelManager` | 多 channel + send / broadcast / dry_run |
| `export/timeline_html.rs` | Timeline HTML 渲染器 | 自包含 HTML（无外链）+ 时间线 + 暗/亮色自适应 + 复制交互 |
| `voice/mod.rs` | Voice 录制 + STT | sox / rec / ffmpeg → OpenAI Whisper API |

**新增 6 个命令**：

```
/channels list / add / remove / toggle / test / broadcast / init / path
/timeline session <name> [out.html] [--theme dark|light|auto]
/timeline demo [out.html] [--theme ...]
/voice check
/voice record [out.wav] [--duration 30] [--m4a]
/voice transcribe <file> [--lang zh|en]
/voice say [out_dir] [--duration 30] [--lang zh] [--m4a]
```

**Round 16 LLM 可见工具增量**（2 个）：

| 工具 | 干啥 |
|---|---|
| `channel_send` | 单发到指定 channel |
| `channel_broadcast` | 广播到所有 channel |

**统计**：100+ 个 .rs 文件，~19400 行，binary ~9.5 MB，**140 tests pass**，0 warning

**下轮（建议顺序）**：

**通道类（OpenClaw 核心：50+）**  ⏸  暂缓
1. **Telegram / Discord / Slack / Signal** ── 等用户提出再做
2. ~~**飞书 / 钉钉 / 企微** ── ✅ Round 16a-d 已交付~~

**Skill/Hook/MCP 类（OpenClaw 5,700+ skills 三个支柱）**
3. ~~**Hooks 系统** —— ✅ Round 4 已交付~~
4. ~~**Skill 系统** —— ✅ Round 5 已交付~~
5. ~~**MCP Streamable HTTP client** —— ✅ Round 6 已交付（Round 15a 补全 Resources + Prompts）~~

**Memory / RAG / Vector 类**
6. ~~**RAG** —— ✅ Round 7 + Round 15b hybrid 已交付~~
7. ~~**SOUL.md 持久身份** —— ✅ Round 13 已交付~~
8. ~~**Heartbeat 主动唤醒** —— ✅ Round 14 已交付~~

**执行/工具 类**
9. ~~**Worktree + 多文件 atomic edit** —— ✅ Round 8 已交付~~
10. ~~**Sandbox 隔离** —— ✅ Round 9 已交付~~
11. ~~**Streaming Markdown** —— ✅ Round 10 已交付~~
12. ~~**Web 控制台** —— ✅ Round 11 已交付~~

**后台任务**
13. ~~**Hermes-style 后台任务引擎** —— ✅ Round 12 已交付~~

**媒体 / 输出**
14. ~~**PPTX 导出** —— ✅ Round 15c 已交付（手写 OOXML）~~
15. ~~**TTS** —— ✅ Round 15d 已交付（macOS `say`）~~
16. ~~**Timeline HTML 导出** —— ✅ Round 16e 已交付~~
17. ~~**Voice 录制 + STT** —— ✅ Round 16f 已交付（Whisper）~~

**集成（OpenClaw 周边生态）**
18. **Skill 市场协议** —— 让别人能 publish 自己的 skill

需要哪个先做，给我编号。

**Round 14 新增**：

| 模块 | 文件 | 干啥 |
|---|---|---|
| `heartbeat/policy.rs` | `HeartbeatPolicy` | 从 SOUL.md 的 `## heartbeat` 段解析 enabled / interval / directives |
| `heartbeat/state.rs` | `HeartbeatState` | 状态持久化（`~/.fr_cli/heartbeat_state.json`，最多 50 条 history） |
| `heartbeat/registry.rs` | `HeartbeatRegistry` | 后台 tick 协程（1 分钟查一次） + `run_now` 手动触发 |
| `heartbeat/runner.rs` | `HeartbeatRunner` | 一次 heartbeat 跑：调 LLM、收集 tool calls、写 long-term 报告 |
| `tools/heartbeat.rs` | `HeartbeatToolContext` + 3 个工具 | `heartbeat_status` / `heartbeat_now` / `heartbeat_set` 给 LLM 用 |

**SOUL.md 段格式**：

```markdown
## heartbeat
enabled: true
interval_minutes: 60
directives:
  - 跑 /tasks list 检查待办
  - 跑 rag_query 复习最近学习笔记
```

**`/heartbeat` 命令**（9 个子命令）：

```
/heartbeat                          status
/heartbeat on / off                 开关
/heartbeat now                      立即触发（async spawn）
/heartbeat interval <N>             设间隔分钟数
/heartbeat directives               列出
/heartbeat directives add <text>    追加（写回 SOUL）
/heartbeat directives clear         清空
/heartbeat history                  最近 10 次跑
/heartbeat reload                   从 SOUL 重读 policy
```

**总览**：88 个 .rs 文件，~16044 行，binary ~9.3 MB，**114 tests pass**，0 warning

**下轮（建议顺序，对标 ZeroClaw / ZeptoClaw 水准）**：

**通道类（OpenClaw 核心：50+）**  ⏸  暂缓
1. **多通讯通道**（Telegram / Discord / 飞书 / Webhook）—— 等用户提出再做

**Skill/Hook/MCP 类（OpenClaw 5,700+ skills 三个支柱）**
2. ~~**Hooks 系统** —— ✅ Round 4 已交付~~
3. ~~**Skill 系统** —— ✅ Round 5 已交付~~
4. ~~**MCP Streamable HTTP client** —— ✅ Round 6 已交付~~

**Memory / RAG / Vector 类**
5. ~~**RAG** —— ✅ Round 7 已交付~~
6. ~~**SOUL.md 持久身份** —— ✅ Round 13 已交付~~
7. ~~**Heartbeat 主动唤醒** —— ✅ Round 14 已交付（SOUL 段驱动 + 1 分钟检查 + directives）~~

**执行/工具 类**
8. ~~**Worktree + 多文件 atomic edit** —— ✅ Round 8 已交付~~
9. ~~**Sandbox 隔离** —— ✅ Round 9 已交付~~
10. ~~**Streaming Markdown** —— ✅ Round 10 已交付~~
11. ~~**Web 控制台** —— ✅ Round 11 已交付~~

**后台任务**
12. ~~**Hermes-style 后台任务引擎** —— ✅ Round 12 已交付~~

**媒体 / 输出**
13. **PPT / Timeline HTML 导出**
14. **TTS / Voice 录制**（接 macOS `say`）

**集成（OpenClaw 周边生态）**
15. **多平台渠道**（WhatsApp / iMessage / Signal）
16. **企业 IM（飞书 / 钉钉 / 企微）**
17. **Skill 市场协议** —— 让别人能 publish 自己的 skill

需要哪个先做，给我编号。

需要哪个先做，直接告诉我编号或描述。

## License

MIT（与 Python 版一致）
