# 关闭前落盘与外部修改冲突检测 实现计划（2026-10-23）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 窗口关闭前保证最新文本落盘；保存时发现外部修改则拒绝覆盖，并提供重新载入/覆盖保存。

**Status:** Completed 2026-10-23（实现、测试、文档门禁全部完成）。

---

## File Structure
- Modify: `src-tauri/src/notes.rs`、`src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src/core/notes.ts`、`src/application/ports.ts`、`src/application/workspace.ts`
- Modify: `src/adapters/tauri/noteRepository.ts`
- Modify: `src/ui/App.tsx`、`src/ui/StatusBar.tsx`、`src/ui/app.css`
- Modify: `src/ui/App.test.tsx`、`src/application/workspace.test.ts`
- Modify: `README.md`、`docs/backlog.md`、`docs/architecture/C3-component.md`、RFC-003 §6/§8

---

### Task 0: 设计与文档
**Files:** RFC-003、ADR-005、spec、plan、ADR 索引、C3

- [x] **Step 1**: 写 RFC-003（REQ-013~016）、ADR-005、spec、plan。
- [x] **Step 2**: 更新 ADR 索引与 C3。
- [x] **Step 3**: `node scripts/check-eng-docs.cjs` PASS。
- [x] **Step 4**: 本次不创建提交（用户未要求）。

### Task 1: Rust revision 与条件写入
**Files:** `src-tauri/src/notes.rs`、`lib.rs`

- [x] **Step 1**: 先写失败测试：读写往返 revision 一致、外部修改 conflict、缺失文件 conflict。
- [x] **Step 2**: 实现 `NoteDocument`、`revision_for_bytes`、`read_note` 新返回、`write_note(expectedRevision) -> revision`。
- [x] **Step 3**: 更新超大内容测试的 `write_note` 参数。
- [x] **Step 4**: `cargo test` 全绿（15 passed）。

### Task 2: 前端类型与保存链
**Files:** `src/core/notes.ts`、ports、service、adapter、App

- [x] **Step 1**: 增加 `NoteDocument`，修改端口/服务签名。
- [x] **Step 2**: App 增加 `revisionRef`，saver 传递 expectedRevision 并更新 revision。
- [x] **Step 3**: 打开/改名/删除/工作区切换同步处理 revision。
- [x] **Step 4**: `bun run typecheck`（PASS）。

### Task 3: 冲突 UI
**Files:** `App.tsx`、`StatusBar.tsx`、`app.css`

- [x] **Step 1**: conflict 状态、横幅、重新载入、覆盖保存。
- [x] **Step 2**: 冲突期间不调度自动保存；flush 失败阻止切换。
- [x] **Step 3**: StatusBar 显示「磁盘已变更」。

### Task 4: 关闭前 flush
**Files:** `App.tsx`、`capabilities/default.json`

- [x] **Step 1**: `getCurrentWindow().onCloseRequested` + `closingRef` + flush。
- [x] **Step 2**: flush 失败保持窗口；成功后再 `close()`。
- [x] **Step 3**: 增加 `core:window:allow-close` 权限。

### Task 5: 测试
**Files:** `App.test.tsx`、`workspace.test.ts`

- [x] **Step 1**: mock `read_note` 新结构、`write_note` 返回值、window API。
- [x] **Step 2**: close flush 成功/失败测试。
- [x] **Step 3**: conflict 显示、重新载入、覆盖保存测试。
- [x] **Step 4**: `bun run test` 全绿（28 passed）。

### Task 6: 全量验证与收尾
**Files:** docs、README

- [x] **Step 1**: `bun run typecheck`、`bun run test`、`bun run build`。
- [x] **Step 2**: `cargo test`、`bun run tauri build`（NSIS）。
- [x] **Step 3**: 文档门禁 PASS；回填 RFC-003 §6/§8。
- [x] **Step 4**: 更新 backlog B1-4/B1-5、README、knowledge。

---

## Self-Review
- 数据语义：冲突时不写入；覆盖保存使用第二次读取的最新 revision；重新载入显式丢弃本地改动。
- 与既有机制共存：关闭/切换复用 flush；revision 不落盘；无后台轮询。
- 回归：前端 28 passed（+5 close/conflict 用例）、Rust 15 passed（+3 revision/conflict 用例）；文档门禁 PASS。
