# RFC-004: 删除进系统回收站与撤销入口（域 Notes）

> Status: Implemented (Completed 2026-10-23) | Type: RFC | Created: 2026-10-23 | Updated: 2026-10-23 | Authors: Buffy | Reviewers: 项目所有者 | Target: v0.4.0
> 索引：`RFC-004` | 关联 REQ：REQ-017~REQ-019 | 收益：误删的笔记可从系统回收站找回，并可在删除后立即一键撤销。
> 位置：`docs/rfc/RFC-004-trash-and-undo.md` 权威；`docs/specs/2026-10-23-trash-and-undo-design.md` 为实现快照
> 一致性声明：本文档与 RFC-002（删除语义）、RFC-003（条件写入）、ADR-003（文件系统是唯一事实源）、ADR-006（系统回收站 + 会话内快照撤销）保持一致；不引入应用内回收站目录或持久化删除历史。

## 0. 更新摘要

2026-10-23：建立 RFC-004。RFC-002 的删除是 `fs::remove_file` 永久删除，backlog B1-7 记录了误删不可恢复。本 RFC 把删除改为系统回收站，并在删除成功后提供会话内的「撤销」入口；撤销通过内存中的内容快照把文件写回原路径。

## 1. 需求

| REQ | 能力 | 基线 | 目标 | 验证 |
|---|---|---|---|---|
| REQ-017 | 删除进系统回收站 | 0：`delete_note` 直接 `fs::remove_file`，永久删除（`src-tauri/src/notes.rs`） | 删除改为移入 Windows/macOS/Linux 系统回收站；回收站失败则文件保持原位并报错 | `cd md-studio/src-tauri && cargo test trash` |
| REQ-018 | 删除后撤销入口 | 0：删除后无任何恢复入口 | 删除成功后显示 toast「已移入系统回收站」，提供「撤销」；撤销把原文写回原路径并重新打开 | `cd md-studio && bun run test undo` |
| REQ-019 | 恢复安全与边界 | 0：无恢复命令 | 恢复不覆盖已存在文件、沿用路径校验与 5 MiB 限制；>5 MiB 或非 UTF-8 文件仍可删除但不提供撤销 | Rust `restore` 单测 + 文档说明 |

## 2. 现状根因

- `src-tauri/src/notes.rs` 的 `delete_note` 调用 `fs::remove_file`，文件不进入回收站。
- 删除命令没有返回值，前端删除后立即 `setOpened(null)`，没有保留任何可恢复的内容。
- `src/ui/App.tsx` 的 `handleDelete` 成功后只刷新列表；`NoteDialog` 文案明确写着「此操作不可恢复」。
- 系统回收站没有跨平台统一的「按路径恢复」API；直接恢复依赖平台实现，不适合作为应用契约。

## 3. 总体设计

```mermaid
flowchart LR
  UI[App 删除确认 / UndoToast] --> SVC[WorkspaceService]
  SVC --> PORT[NoteRepository]
  PORT --> ADAPTER[TauriNoteRepository]
  ADAPTER -->|delete_note| RUST[Rust notes.rs]
  RUST -->|trash::delete| OS[(系统回收站)]
  RUST -->|DeletedNote path + content| ADAPTER
  ADAPTER --> UI
  UI -->|restore_note(path, content)| ADAPTER
  ADAPTER --> RUST
  RUST -->|fs::write 原路径| FS[(工作区文件)]
```

不变量：

1. 删除的落点只有一个：文件进入系统回收站，应用不保留第二份磁盘副本。
2. 撤销只针对「最近一次删除、当前会话」，内容快照保存在前端内存；应用退出后撤销入口消失。
3. 恢复只写原相对路径；目标已存在时拒绝，绝不覆盖现有文件。
4. 回收站失败时删除失败，文件仍在原路径。

## 4. 详细设计

### 4.1 Rust：回收站删除与快照（REQ-017、REQ-019）

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedNote {
    pub path: String,
    /// 会话内撤销用的文本快照；>5 MiB 或非 UTF-8 时为 None。
    pub content: Option<String>,
}

pub fn delete_note(root: String, path: String) -> AppResult<DeletedNote>;
pub fn restore_note(root: String, path: String, content: String) -> AppResult<NoteEntry>;
```

- `delete_note`：`resolve_existing` → 读取字节（失败则中止，不删除）→ 超过 `MAX_FILE_BYTES` 或非 UTF-8 时 `content = None` → `trash::delete(&target)` → 返回相对路径与快照。
- 删除使用 `trash` crate（跨平台封装 Windows Shell、macOS Finder、freedesktop Trash）。失败返回 `cannot move note to trash: …`。
- `restore_note`：内容大小检查 → `canonical_root` → `resolve_target`；目标存在则返回 `note path is already occupied` → 写入 → 返回 `NoteEntry`。
- 为了单测不污染真实回收站，删除逻辑写成 `delete_note_with(root, path, trash_fn)`，命令层传入 `trash::delete` 的适配闭包。
- `lib.rs` 注册 `restore_note`。

### 4.2 前端：删除与撤销（REQ-018）

- `DeletedNote { path, content: string | null }` 加入 `src/core/notes.ts`；端口/服务/adapter 增加 `deleteNote -> DeletedNote` 与 `restoreNote(path, content) -> NoteEntry`。
- `handleDelete` 成功后：关闭对话框、清空当前笔记、刷新列表，并 `setUndo(deleted)`。
- `UndoToast`（App 内联）：底部浮层，文案「已移入系统回收站：<文件名>」；`content` 非空时显示「撤销」，另有关闭按钮。
- 撤销：`restoreNote(root, undo.path, undo.content)` → 刷新列表 → `openNote(restored)` → 清空 undo；失败显示错误并保留 toast 以便重试。
- toast 8 秒后自动消失；切换工作区、再次删除会替换当前 undo。
- `NoteDialog` 删除文案改为「文件会移入系统回收站，删除后可立即撤销」。

### 4.3 平台与权限

- 不需要新增 Tauri 能力：`trash` crate 在 Rust 进程内调用系统 API。
- Windows 回收站对文件大小/路径长度有限制，超限时由 `trash::delete` 返回错误，UI 显示保存/删除失败。
- 不引入回收站浏览/清空功能；需要找回更早的文件时使用系统回收站。

## 5. 错误处理与可观测性

| 失败模式 | 处理 |
|---|---|
| 回收站调用失败 | 返回错误，文件保持原位；对话框显示错误，不允许假装删除成功 |
| 删除前读取失败 | 中止删除，保留文件；错误条说明原因 |
| 撤销时原路径已被占用 | 返回 `note path is already occupied`；toast 保留，错误条提示 |
| 撤销内容超过 5 MiB | Rust 拒绝写入；toast 保留 |
| 撤销后重新打开失败 | 文件已恢复，错误条提示，列表仍刷新 |
| 删除 >5 MiB / 非 UTF-8 文件 | 仍可删除进回收站；`content = null`，toast 不显示撤销 |

不引入遥测；错误条、toast 与系统回收站本身是唯一的可观测面。

## 6. 验收

| REQ | 验收结果 |
|---|---|
| REQ-017 | **通过 2026-10-23**：`delete_note` 改用 `trash::delete`；Rust `delete_keeps_the_file_when_the_trash_call_fails` 验证回收站失败时文件保持原位，`delete_returns_snapshot_and_moves_note_out_of_the_workspace` 验证快照与移除 |
| REQ-018 | **通过 2026-10-23**：`App.test.tsx` 删除后显示「已移入系统回收站」，点击「撤销」调用 `restore_note` 并重新打开编辑器 |
| REQ-019 | **通过 2026-10-23**：Rust `restore_note_writes_the_snapshot_back`、`restore_note_refuses_to_overwrite_an_existing_file`、`restore_note_rejects_traversal_and_oversized_content`；`delete_returns_no_snapshot_for_invalid_utf8` 覆盖无快照分支 |

## 7. 非目标

- 应用内回收站目录 / 软删除索引：与 ADR-003「文件系统是唯一事实源」冲突，且会复制正文。
- 持久化删除历史与跨会话撤销：需要数据库或日志，留待有需求时开新 ADR。
- 回收站浏览、批量恢复、清空：交给操作系统。
- 恢复文件元数据（mtime、ACL）：恢复只保证内容与路径。
- 撤销重命名/覆盖保存：本 RFC 只覆盖删除。

## 8. 追溯

- 实现：`src-tauri/Cargo.toml`（`trash 5.2.9`）、`src-tauri/src/notes.rs`（`DeletedNote`、`delete_note_with`、`restore_note`）、`src/ui/App.tsx`（undo 状态、撤销 toast）、`src/ui/NoteDialog.tsx`、`src/ui/app.css`。
- 测试：前端 30 passed（新增 2 个删除/撤销用例，NoteDialog 文案断言更新）；Rust 21 passed（新增 6 个 trash/restore 用例）。回收站调用在单测中通过闭包替换，避免污染真实回收站。
- 真实回收站冒烟：额外执行一次性测试，调用 `delete_note` 把临时文件移入 Windows 回收站，确认文件消失且 `trash::os_limited::list` 可见，随后 `purge_all` 清理；验证后移除该一次性测试。
- 命令：`bun run typecheck`、`bun run test`、`bun run build`、`cargo test --locked --all-targets`、`bun run tauri build`（NSIS 1.52 MiB）、`node scripts/check-eng-docs.cjs` 全部通过。
- 实现快照：`docs/specs/2026-10-23-trash-and-undo-design.md`；计划：`docs/plans/2026-10-23-trash-and-undo-plan.md`。
- 本次未创建提交（用户未要求）。
