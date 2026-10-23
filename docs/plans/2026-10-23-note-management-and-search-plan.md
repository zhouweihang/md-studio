# 笔记删除、重命名与全文搜索 实现计划（2026-10-23）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 MD Studio 内完成笔记删除、同目录重命名与正文搜索，并保持路径安全、自动保存一致性和现有分层边界。

**Status:** 已实现并验证：前端 27 passed、Rust 12 passed、typecheck/build/docs PASS。提交未执行（用户未要求）。

---

## File Structure
- Modify: `src-tauri/src/notes.rs`（命令 + 单测）
- Modify: `src-tauri/src/lib.rs`（注册命令）
- Modify: `src/application/ports.ts`、`src/application/workspace.ts`
- Create: `src/core/search.ts`
- Modify: `src/adapters/tauri/noteRepository.ts`
- Create: `src/ui/NoteDialog.tsx`、`src/ui/NoteDialog.test.tsx`
- Modify: `src/ui/App.tsx`、`src/ui/Sidebar.tsx`、`src/ui/EditorPane.tsx`、`src/ui/app.css`
- Modify: `src/ui/App.test.tsx`
- Modify: `README.md`、`docs/backlog.md`、`docs/architecture/C3-component.md`、RFC-002 §6/§8

---

### Task 0: 设计与文档
**Files:** `docs/rfc/RFC-002-*.md`、`docs/adr/004-*.md`、spec/plan、ADR 索引、C3

- [x] **Step 1**: 写 RFC-002（REQ-008~012）、ADR-004、spec、plan。
- [x] **Step 2**: 更新 ADR 索引与 C3 追溯/数据流。
- [x] **Step 3**: `node scripts/check-eng-docs.cjs` PASS。
- [x] **Step 4**: 本次不创建提交（用户未要求）；改动保留在工作区。

### Task 1: Rust 删除/改名/搜索
**Files:** `src-tauri/src/notes.rs`、`lib.rs`

- [x] **Step 1**: 先写失败测试：删除 + 路径拒绝、改名 + 扩展名保留 + 冲突、搜索 + 大小写 + 行号 + 空查询。
- [x] **Step 2**: 实现 `delete_note`、`rename_note`、`search_notes` 与 `SearchHit`。
- [x] **Step 3**: `sanitize_title` 支持剥离三种 Markdown 扩展名。
- [x] **Step 4**: `run()` 注册三个新命令。
- [x] **Step 5**: `cargo test` 全绿（12 passed）。

### Task 2: 端口与适配器
**Files:** `src/application/*`、`src/core/search.ts`、`src/adapters/tauri/noteRepository.ts`

- [x] **Step 1**: 新增 `SearchHit` 类型与端口方法。
- [x] **Step 2**: `WorkspaceService` 透传 `delete/rename/search`。
- [x] **Step 3**: Tauri 适配器映射 `delete_note` / `rename_note` / `search_notes`。
- [x] **Step 4**: `bun run typecheck` 通过。

### Task 3: 搜索 UI
**Files:** `src/ui/App.tsx`、`Sidebar.tsx`、`EditorPane.tsx`、`app.css`

- [x] **Step 1**: `App` 增加 250ms 防抖搜索与过期响应丢弃。
- [x] **Step 2**: `Sidebar` 增加「内容匹配」分组（路径、行号、片段）并保留文件名树。
- [x] **Step 3**: 点击命中打开笔记并把编辑器定位到行。
- [x] **Step 4**: 更新 App 测试，覆盖内容命中。

### Task 4: 删除/改名 UI
**Files:** `src/ui/NoteDialog.tsx`、`App.tsx`、`app.css`

- [x] **Step 1**: 实现对话框（rename 输入、delete 确认、Escape/Enter、busy）。
- [x] **Step 2**: 工具栏接入；删除前 cancel saver，改名成功后重建 saver。
- [x] **Step 3**: 增加 UI 测试覆盖 rename 与 delete 流程。
- [x] **Step 4**: `bun run typecheck` 与 `bun run build` 通过。

### Task 5: 全量验证
**Files:** 无新增

- [x] **Step 1**: `bun run test` —— 7 files / 27 passed。
- [x] **Step 2**: `cargo test` —— 12 passed。
- [x] **Step 3**: `bun run typecheck`、`bun run build` 通过。
- [x] **Step 4**: `node scripts/check-eng-docs.cjs` PASS。

### Task 6: 收尾
**Files:** `docs/rfc/RFC-002-*.md`、`docs/backlog.md`、`README.md`、`knowledge.md`

- [x] **Step 1**: 回填 RFC-002 §6/§8。
- [x] **Step 2**: 勾掉 backlog B1-1/B1-2，记录完成日期与证据。
- [x] **Step 3**: README 功能与限制更新。
- [x] **Step 4**: 最终文档门禁。

---

## Self-Review
- 数据语义：删除是永久 `remove_file`；改名不复制不重写正文；搜索只读。
- 与既有机制共存：改名/删除前后自动保存状态必须显式处理；搜索不建索引。
- 回归：现有 21 个前端测试、7 个 Rust 测试保持通过；新增测试覆盖新路径；文档门禁 PASS。
