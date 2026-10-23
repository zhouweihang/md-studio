# 搜索命中高亮与大工作区性能测试 实现计划（2026-10-23）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 内容命中片段高亮查询词；用 1,000 篇笔记的 Rust 测试守住搜索耗时。

**Status:** Completed 2026-10-23（实现、测试、文档门禁全部完成）。

---

## File Structure
- Modify: `src/core/search.ts`、`src/ui/Sidebar.tsx`、`src/ui/app.css`
- Add test: `src/core/search.test.ts`
- Modify: `src/ui/App.test.tsx`
- Modify: `src-tauri/src/notes.rs`
- Modify: `README.md`、`docs/backlog.md`、`docs/architecture/C3-component.md`、RFC-005 §6/§8

---

### Task 0: 设计与文档
**Files:** RFC-005、spec、plan、C3

- [x] **Step 1**: 写 RFC-005（REQ-020~021）、spec、plan。
- [x] **Step 2**: 更新 C3。
- [x] **Step 3**: `node scripts/check-eng-docs.cjs` PASS。
- [x] **Step 4**: 本次不创建提交（用户未要求）。

### Task 1: 高亮纯函数
**Files:** `src/core/search.ts`、`src/core/search.test.ts`

- [x] **Step 1**: 先写测试：大小写、多匹配、中文、空 query、无匹配、拼接还原。
- [x] **Step 2**: 实现 `HighlightPart` 与 `highlightParts`。
- [x] **Step 3**: `bun run test search` 全绿（5 passed）。

### Task 2: Sidebar 高亮渲染
**Files:** `Sidebar.tsx`、`app.css`、`App.test.tsx`

- [x] **Step 1**: 用 parts 渲染 `<mark className="hit-mark">`。
- [x] **Step 2**: 增加 `.hit-mark` 样式。
- [x] **Step 3**: App 集成测试：mock `search_notes`，断言 `<mark>` 文本。
- [x] **Step 4**: `bun run typecheck && bun run test` 全绿（36 passed）。

### Task 3: Rust 大工作区性能测试
**Files:** `src-tauri/src/notes.rs`

- [x] **Step 1**: 创建 1,000 篇笔记 + 1 篇唯一命中。
- [x] **Step 2**: 计时 `search_notes_blocking`，断言命中与 10s 预算，输出耗时。
- [x] **Step 3**: `cargo test large_workspace -- --nocapture` 通过并记录实测（330.97ms）。

### Task 4: 全量验证与收尾

- [x] **Step 1**: `bun run typecheck`、`bun run test`、`bun run build`。
- [x] **Step 2**: `cargo test --locked --all-targets`、`bun run tauri build`（NSIS 1.52 MiB）。
- [x] **Step 3**: 文档门禁 PASS；回填 RFC-005 §6/§8。
- [x] **Step 4**: 更新 backlog、README、knowledge、C3。

---

## Self-Review
- 数据语义：高亮只改变渲染切分，preview 文本不变；搜索契约不变。
- 与既有机制共存：文件名过滤、内容命中、点击定位均保持不变；性能测试使用临时目录。
- 回归：前端 36 passed（+6）、Rust 22 passed（+1）；文档门禁 PASS。
