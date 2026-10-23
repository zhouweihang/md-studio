# 系统回收站删除与撤销 设计快照

> 索引：`RFC-004` | 关联 REQ：REQ-017~REQ-019 | 收益：误删可找回，删除后可立即撤销
> 位置：本文档为实现快照；权威见 `docs/rfc/RFC-004-trash-and-undo.md`

## 1. 需求

见 RFC-004 §1。实现范围：`trash` 删除、删除快照、`restore_note`、App 撤销 toast、对话框文案、测试。不做应用内回收站、持久化历史、回收站浏览。

## 2. 现状根因

- `src-tauri/src/notes.rs` 的 `delete_note` 是 `fs::remove_file`，永久删除且无返回值。
- `src/ui/App.tsx` 的 `handleDelete` 成功后清空当前笔记，没有可恢复数据。
- `src/ui/NoteDialog.tsx` 删除确认文案写明「此操作不可恢复」。

## 3. 总体设计

指向 RFC-004 §3；实现落点：

```text
src-tauri/Cargo.toml                 trash = "5"
src-tauri/src/notes.rs               DeletedNote、delete_note_with、trash 适配、restore_note
src-tauri/src/lib.rs                 注册 restore_note
src/core/notes.ts                    DeletedNote 类型
src/application/ports.ts             deleteNote -> DeletedNote；restoreNote
src/application/workspace.ts         透传
src/adapters/tauri/noteRepository.ts invoke 映射
src/ui/App.tsx                       undo 状态、撤销 toast、handleUndoDelete
src/ui/NoteDialog.tsx                删除文案
src/ui/app.css                       undo-toast 样式
```

## 4. 详细设计（实现视角增量）

### 4.1 Rust

- `delete_note_with<F>(root, path, trash_fn)`：
  - `canonical_root` → `resolve_existing`；
  - 读取字节：失败则返回错误，不删除；
  - `content = (len <= MAX_FILE_BYTES).then(|| String::from_utf8(bytes).ok()).flatten()`；
  - `trash_fn(&target)`；失败 → `cannot move note to trash: …`；
  - 返回 `DeletedNote { path: to_relative(root, target), content }`。
- 命令 `delete_note` 用 `|path| trash::delete(path).map_err(|e| e.to_string())` 调用。
- `restore_note(root, path, content)`：大小检查 → `resolve_target` → `target.exists()` 则 `note path is already occupied` → `fs::write` → `entry_for`。
- 单测用闭包模拟回收站（`fs::remove_file` 或返回错误），不触碰真实回收站。

### 4.2 前端

- `DeletedNote { path: string; content: string | null }`。
- `handleDelete` 成功后 `setUndo(deleted)`；toast 中 `content !== null` 才渲染「撤销」。
- `handleUndoDelete`：`service.restore` → `service.list` → `openNote(restored)` → `setUndo(null)`。
- `useEffect` 在 undo 变化后 8 秒清空。
- `openNote` 前无需 flush（删除已清空当前稿）；`revisionRef` 由 `openNote` 重新读取设置。
- 错误映射：`already occupied` → 「原路径已被其他文件占用，无法撤销。」；`cannot move note to trash` → 保留原文。

### 4.3 测试

- Rust：删除返回内容快照且文件从磁盘消失（模拟回收站）；回收站失败时文件保持；非 UTF-8 快照为 None；恢复写回原文并返回条目；恢复拒绝已存在路径与路径越界。
- App：删除确认后显示 toast「已移入系统回收站」；点击「撤销」调用 `restore_note` 并重新打开笔记；`content: null` 时不显示撤销按钮。
- Mock：`delete_note` 返回 `{ path, content }`；`restore_note` 返回 `NoteEntry`。

## 5. Self-Review

- 数据语义：删除只有系统回收站一个落点；撤销写回原路径且不覆盖；快照不落盘。
- 与既有机制共存：删除仍先 `saver.cancel()`；恢复后走正常 `openNote` 读取 revision；搜索/列表无需特殊处理。
- 已知边界：会话内、仅最近一次、快照上限 5 MiB；恢复不保留元数据。
