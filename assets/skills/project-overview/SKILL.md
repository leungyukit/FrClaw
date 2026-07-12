---
name: project-overview
description: 给当前项目生成一份 module 概览（用 read_file + list_dir 看代码结构）
triggers:
  - "项目概览"
  - "module 概览"
  - "代码结构"
  - "介绍下这个项目"
  - "@overview"
allowed-tools:
  - read_file
  - list_dir
max-steps: 6
---

# Project Overview Skill

## Step 1 — 抓总览
- `read_file({path: "Cargo.toml"})` 或 `package.json` / `pyproject.toml` 等
- `list_dir({path: "."})` 看根目录

## Step 2 — 读 README
- `read_file({path: "README.md"})` 提取项目目的

## Step 3 — 扫 src/ 树
- `list_dir({path: "src"})` 看模块切分
- 对每个顶层模块，必要时 `read_file` 抓一段 public API / 类型签名

## Step 4 — 总结
按以下结构输出：
1. **项目目的**（一句）
2. **技术栈**
3. **模块结构**（每模块一句话）
4. **关键入口 / 工具点**
