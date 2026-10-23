# 笔记删除、重命名与全文搜索 设计快照

> 索引：`RFC-002` | 关联 REQ：REQ-008~REQ-012 | 收益：在应用内完成查找与整理，搜索覆盖正文
> 位置：本文档为实现快照；权威见 `docs/rfc/RFC-002-note-management-and-search.md`

## 1. 需求

见 RFC-002 §1。实现范围：确认后永久删除、同目录改名、Rust 实时正文搜索、工具栏操作与搜索结果 UI。不引入回收站、正则、索引。

## 2. 现状根因

- `src/application/ports.ts:3` 的端口只有读取/写入/创建，没有删除/改名/搜索。
- `src-tauri/src/notes.rs` 只注册四个命令；没有 `remove_file`/`rename`/搜索实现。
- `src/ui/Sidebar.tsx:99` 的搜索只过滤路径与标题，不读取正文。
- `src/ui/App.tsx` 的 saver 闭包绑定打开时的路径；改名后不重建会写旧路径。
- 没有确认/重命名对话框，工具栏没有笔记级操作。

## 3. 总体设计

指向 RFC-002 §3 的流程图；实现落点：

```text
src-tauri/src/notes.rs       delete_note / rename_note / search_notes + tests
src/application/ports.ts     NoteRepository 扩展三个方法
src/application/workspace.ts WorkspaceService 透传
src/adapters/tauri/noteRepository.ts  invoke 映射
src/core/search.ts           SearchHit 类型
src/ui/NoteDialog.tsx        rename/delete 对话框
src/ui/App.tsx               工具栏、搜索防抖、删除/改名编排
src/ui/Sidebar.tsx           内容命中分组
src/ui/EditorPane.tsx        按行定位选区
```

## 4. 详细设计（实现视角增量）

### 4.1 Rust

- `delete_note(root, path)`：`canonical_root` → `resolve_existing` → `fs::remove_file`。
- `rename_note(root, path, new_title) -> NoteEntry`：
  - 源目录固定；扩展名取源文件；
  - `sanitize_title` 扩展为剥离 `.md/.markdown/.mdown`；
  - 目标存在且 `canonicalize(target) != source` → 冲突错误；
  - `fs::rename` 后返回 `entry_for`。
- `search_notes(root, query)`：`async` + `spawn_blocking`；空查询返回空；`list_notes` 扫描后逐篇 `read_to_string`，逐行大小写不敏感匹配；每篇 3 条、总计 50 条；`SearchHit { path, title, line, preview }`。
- 常量：`MAX_SEARCH_HITS = 50`、`MAX_HITS_PER_NOTE = 3`、预览约 90 字符。

### 4.2 前端数据流

- 端口新增 `deleteNote/renameNote/searchNotes`；服务只做委托。
- 搜索：`App` 在 `query` 非空时 250ms 防抖调用 `searchNotes`；用 `cancelled` 标记丢弃旧响应；`Sidebar` 同时展示文件名树与内容命中。
- 点击命中：`openNote(note, line)` 将 `focusLine` 传入 `EditorPane`；编辑器按行计算字符偏移并选中该行。
- 删除：`saver.cancel()` → `deleteNote` → 刷新列表、关闭笔记、`saver.dispose()`。
- 改名：`flushCurrent()` → `renameNote` → 刷新列表 → `openNote(updated)` 重建 saver。
- 对话框：`NoteDialog` 的 rename 模式 Enter 提交、Escape 取消；delete 模式只允许确认/取消。

### 4.3 陷阱

- **不能在 flush 前改名**：saver 仍指向旧路径。
- **不能在删除后保留定时器**：防抖回调可能把文件重新写出来。
- **搜索响应可能乱序**：必须用请求代次/取消标记丢弃旧结果。
- **Windows 大小写不敏感**：目标存在检查用 `canonicalize` 比较源，避免自我冲突。
- **Unicode 预览**：按 `chars()` 截断，不能按字节切。

## 5. Self-Review

- 数据语义：删除永久、改名不复制内容、搜索只读；不改变换行与正文。
- 与既有机制共存：仍以文件系统为事实源；不建索引；自动保存路径在改名/删除时显式处理。
- 回归：现有 21 个前端测试与 7 个 Rust 测试必须继续通过；新增搜索/删除/改名测试；文档门禁 PASS。
