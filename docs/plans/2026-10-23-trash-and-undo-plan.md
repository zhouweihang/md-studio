# 系统回收站删除与撤销 实现计划（2026-10-23）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除改为系统回收站；删除成功后提供会话内撤销，把原文写回原路径。

**Status:** Completed 2026-10-23（实现、测试、文档门禁全部完成）。

---

## File Structure
- Add dependency: `src-tauri/Cargo.toml`（`trash`）
- Modify: `src-tauri/src/notes.rs`、`src-tauri/src/lib.rs`
- Modify: `src/core/notes.ts`、`src/application/ports.ts`、`src/application/workspace.ts`
- Modify: `src/adapters/tauri/noteRepository.ts`
- Modify: `src/ui/App.tsx`、`src/ui/NoteDialog.tsx`、`src/ui/app.css`
- Modify: `src/ui/App.test.tsx`、`src/application/workspace.test.ts`
- Modify: `README.md`、`docs/backlog.md`、`docs/architecture/C3-component.md`、RFC-004 §6/§8

---

### Task 0: 设计与文档
**Files:** RFC-004、ADR-006、spec、plan、ADR 索引、C3

- [x] **Step 1**: 写 RFC-004（REQ-017~019）、ADR-006、spec、plan。
- [x] **Step 2**: 更新 ADR 索引与 C3。
- [x] **Step 3**: `node scripts/check-eng-docs.cjs` PASS。
- [x] **Step 4**: 本次不创建提交（用户未要求）。

### Task 1: Rust 回收站删除
**Files:** `src-tauri/Cargo.toml`、`src-tauri/src/notes.rs`、`lib.rs`

- [x] **Step 1**: 先写失败测试：删除返回快照、文件从磁盘消失、回收站失败保留文件。
- [x] **Step 2**: 增加 `trash` 依赖与 `DeletedNote`、`delete_note_with`、命令适配。
- [x] **Step 3**: `cargo test` 全绿（21 passed）。

### Task 2: Rust 恢复命令
**Files:** `src-tauri/src/notes.rs`、`lib.rs`

- [x] **Step 1**: 测试：恢复写回原文、拒绝已存在路径、拒绝越界、超大内容拒绝。
- [x] **Step 2**: 实现 `restore_note` 并注册命令。
- [x] **Step 3**: `cargo test` 全绿（21 passed）。

### Task 3: 前端端口与撤销 UI
**Files:** `core/notes.ts`、ports、service、adapter、`App.tsx`、`NoteDialog.tsx`、`app.css`

- [x] **Step 1**: `DeletedNote` 类型与端口/服务/adapter 签名。
- [x] **Step 2**: 删除成功后显示 undo toast；8 秒自动消失。
- [x] **Step 3**: 撤销恢复并重新打开笔记；错误映射。
- [x] **Step 4**: 更新删除确认文案与 toast 样式。

### Task 4: 前端测试
**Files:** `App.test.tsx`、`workspace.test.ts`

- [x] **Step 1**: mock `delete_note`/`restore_note`，更新 workspace 端口测试。
- [x] **Step 2**: 删除显示 toast；撤销调用恢复并重新打开；无快照时不显示撤销。
- [x] **Step 3**: `bun run typecheck && bun run test` 全绿（30 passed）。

### Task 5: 全量验证与收尾

- [x] **Step 1**: `bun run build`。
- [x] **Step 2**: `cargo test --locked --all-targets`、`bun run tauri build`（NSIS 1.52 MiB）。
- [x] **Step 3**: 文档门禁 PASS；回填 RFC-004 §6/§8。
- [x] **Step 4**: 更新 backlog B1-7、README、knowledge、C3。

---

## Self-Review
- 数据语义：删除只落入系统回收站；撤销写回原路径且拒绝覆盖；快照只在内存。
- 与既有机制共存：删除仍先取消 saver；恢复后正常读取 revision；无新增持久化状态。
- 回归：前端 30 passed（+2 删除/撤销用例）、Rust 21 passed（+6 trash/restore 用例）；文档门禁 PASS。
