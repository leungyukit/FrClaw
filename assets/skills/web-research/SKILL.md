---
name: web-research
description: 用 web 搜索 + 摘要 + 写入长期记忆，研究一个主题
triggers:
  - "研究"
  - "调研"
  - "web search"
  - "查一下"
  - "搜索"
  - "@research"
allowed-tools:
  - web_search
  - memorize
  - recall
max-steps: 8
---

# Web Research Skill

## Step 1 — 拆解 query
如果用户问的是模糊的「最近 xxx 怎么样」，先用 `recall({query: "..."})` 查长期记忆，
避免重复研究。

## Step 2 — 多个角度搜索
对核心主题，跑 2-3 次 `web_search({query, k: 3})`：
- 用「时间」修饰词（"2026"、"最近"）
- 换关键词（同义词 / 上下游概念）

## Step 3 — 整合 + 摘要
把命中按可信度排（带日期 > 无日期；权威源 > 论坛）：
- 重复信息用「约 X 提到...」合并
- 矛盾信息并列展示

## Step 4 — 写入长期记忆（如果用户说「记住」/「长期记」）
调用 `memorize({content: "...", source: "web-research"})` 把可复用的发现沉淀。
否则只在当前对话回答。

## Step 5 — 回答
用 markdown 写要点 + 链接，避免长篇大论。
