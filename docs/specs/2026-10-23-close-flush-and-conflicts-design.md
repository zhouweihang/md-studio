# 关闭前落盘与外部修改冲突检测 设计快照

> 索引：`RFC-003` | 关联 REQ：REQ-013~REQ-016 | 收益：关闭不丢稿，外部修改不被静默覆盖
> 位置：本文档为实现快照；权威见 `docs/rfc/RFC-003-close-flush-and-conflicts.md`

## 1. 需求

见 RFC-003 §1。实现范围：close-request flush、`NoteDocument` revision、条件写入、冲突横幅的重新载入/覆盖保存、测试。不做后台轮询、文件锁、合并。

## 2. 现状根因

- `src-tauri/src/notes.rs` 的 `write_note` 无条件 `fs::write`；`read_note` 只返回 `String`。
- `src/ui/App.tsx` 的 saver 闭包没有 revision；自动保存会覆盖外部修改。
- 没有 `onCloseRequested` 监听；关闭窗口可能丢失防抖窗口内文本。
- `StatusBar` 只有 idle/pending/saved/error 四态；没有冲突态。

## 3. 总体设计

指向 RFC-003 §3；实现落点：

```text
src-tauri/src/notes.rs       NoteDocument、revision_for_bytes、write_note(expected_revision)
src/core/notes.ts            NoteDocument 类型
src/application/ports.ts     readNote -> NoteDocument；writeNote(..., expectedRevision) -> revision
src/application/workspace.ts 透传
src/adapters/tauri/noteRepository.ts  invoke 参数映射
src/ui/App.tsx               revisionRef、conflict 状态、close-request、reload/overwrite
src/ui/StatusBar.tsx         conflict 状态
src/ui/app.css               conflict banner
```

## 4. 详细设计（实现视角增量）

### 4.1 Rust

- `revision_for_bytes(bytes: &[u8]) -> String`：FNV-1a 64 位，输出 16 位十六进制。
- `NoteDocument { content, revision }`，`#[serde(rename_all = "camelCase")]`。
- `read_note`：大小检查 → 读取字节 → UTF-8 解码 → 返回文档。
- `write_note(root, path, content, expected_revision)`：
  - 内容大小检查；
  - `resolve_target`；
  - 目标不存在 → `conflict: note no longer exists`；
  - 读取当前字节并算 revision，与 expected 不同 → `conflict: note was modified outside MD Studio`；
  - 写入并返回新 revision。
- 保留 rename/delete/search 不变；search 用 `read_to_string` 不关心 revision。

### 4.2 前端

- `OpenedNote` 不存 revision；`revisionRef = useRef<string | null>(null)` 保存当前版本。
- `createSaver` 调用 `service.save(root, path, text, revisionRef.current ?? '')`，成功后更新 ref 与 `savedContent`。
- `openNote`/重新载入设置 ref；改名保持 ref；删除/工作区切换清空。
- 冲突 catch：`describeError(caught).startsWith('conflict')` → `setConflict(true)`、`saveState='conflict'`；其他错误走原逻辑。
- `handleChange` 在 conflict 时只更新内存，不 `schedule`。
- `flushCurrent` 在冲突时失败，从而阻止切换/关闭。
- 重新载入：`saver.cancel()` → `read` → 替换内容与 revision → 重建 saver → 清 conflict。
- 覆盖保存：`read` 取最新 revision → `save(localContent, latestRevision)` → 清 pending/conflict → 更新 savedContent/revision。
- 关闭：`getCurrentWindow().onCloseRequested`；`closingRef` 防重入；flush 成功才 `close()`。
- 窗口能力增加 `core:window:allow-close`。

### 4.3 测试

- Rust：revision 读写往返、外部修改触发 conflict 且磁盘保持外部内容、缺失文件 conflict、超大内容仍拒绝。
- App：close 请求 flush 后 close；写失败时不 close；冲突时显示横幅；重新载入读取磁盘版本；覆盖保存使用最新 revision。
- Mock 更新：`read_note` 返回 `{ content, revision }`；`write_note` 返回新 revision 字符串；`@tauri-apps/api/window` 的 `onCloseRequested`/`close`。

## 5. Self-Review

- 数据语义：冲突时不写入；重新载入丢弃本地改动是显式操作；覆盖保存使用最新 revision。
- 与既有机制共存：关闭/切换/改名仍复用 `flushCurrent`；revision 不持久化，外部修改在下次保存时发现。
- 回归：现有 27 个前端测试与 12 个 Rust 测试继续通过；新增 close/conflict 测试；文档门禁 PASS。
