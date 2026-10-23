# RFC-003: 关闭前落盘与外部修改冲突检测（域 Persistence）

> Status: Implemented (Completed 2026-10-23) | Type: RFC | Created: 2026-10-23 | Updated: 2026-10-23 | Authors: Buffy | Reviewers: 项目所有者 | Target: v0.3.0
> 索引：`RFC-003` | 关联 REQ：REQ-013~REQ-016 | 收益：关闭窗口不再丢防抖窗口内的输入；外部编辑器改过文件时，保存不会静默覆盖。
> 位置：`docs/rfc/RFC-003-close-flush-and-conflicts.md` 权威；`docs/specs/2026-10-23-close-flush-and-conflicts-design.md` 为实现快照
> 一致性声明：本文档与 RFC-001（自动保存）、RFC-002（文件操作）、ADR-003（文件系统事实源）、ADR-005（内容版本乐观并发）保持一致；不引入文件锁或后台轮询。

## 0. 更新摘要

2026-10-23：建立 RFC-003。RFC-002 的保存仍是「直接覆盖」，关闭窗口也没有拦截；本 RFC 把保存改为带内容版本的乐观并发，并在窗口关闭前 flush。冲突只提供「重新载入」与「覆盖保存」两个明确选择，不做自动合并。

## 1. 需求

| REQ | 能力 | 基线 | 目标 | 验证 |
|---|---|---|---|---|
| REQ-013 | 关闭窗口前落盘 | 0：关闭窗口不会等待 700ms 防抖（backlog B1-5） | 拦截 close-request，先 flush 最新内容；flush 失败/冲突则保持窗口打开并提示 | `cd md-studio && bun run test close` |
| REQ-014 | 读取返回内容版本 | 0：`read_note` 只返回字符串（`src-tauri/src/notes.rs`） | 返回 `{ content, revision }`；revision 为文件字节的稳定指纹 | `cd md-studio/src-tauri && cargo test revision` |
| REQ-015 | 保存时检测外部修改 | 0：`write_note` 无条件覆盖（backlog B1-4） | 写入前比较磁盘 revision；不一致返回 `conflict:` 错误，不覆盖 | `cd md-studio/src-tauri && cargo test conflict` |
| REQ-016 | 冲突 UI 与验证 | 0：无冲突状态、无恢复入口 | 冲突横幅提供「重新载入」「覆盖保存」；自动保存暂停；切换/关闭被阻止直到解决 | `cd md-studio && bun run test conflict` |

## 2. 现状根因

- `src-tauri/src/notes.rs` 的 `write_note` 直接 `fs::write`，没有读取当前磁盘内容，无法发现外部修改。
- `read_note` 返回 `String`，前端没有可用于后续写回比较的版本标识。
- `src/ui/App.tsx` 的 `createSaver` 闭包只保存路径与文本；外部修改后下一次自动保存会覆盖磁盘。
- `src/ui/App.tsx` 没有注册 `getCurrentWindow().onCloseRequested`，关闭窗口时防抖计时器可能尚未触发。
- 现有 `DebouncedSaver.flush()` 已能在切换笔记/Ctrl+S 时落盘，可直接复用于关闭流程。

## 3. 总体设计

```mermaid
flowchart LR
  UI[App / StatusBar / ConflictBanner] --> SVC[WorkspaceService]
  SVC --> PORT[NoteRepository]
  PORT --> ADAPTER[TauriNoteRepository]
  ADAPTER -->|read_note| RUST[Rust notes.rs]
  ADAPTER -->|write_note(content, expectedRevision)| RUST
  RUST -->|NoteDocument content + revision| ADAPTER
  RUST -->|conflict: ...| ADAPTER
  ADAPTER --> UI
  UI -->|close-request| WIN[Tauri Window]
  WIN -->|flush then close| UI
```

不变量：

1. 每次读取生成 revision；保存必须携带读取时/上次成功保存后的 revision。
2. Rust 在写入前重新读取磁盘并计算 revision；不一致直接拒绝。
3. 冲突时本地内容保留在内存，自动保存停止，用户显式选择恢复策略。
4. 关闭/切换笔记先 flush；flush 失败（含冲突）则不继续。

## 4. 详细设计

### 4.1 内容版本与条件写入（REQ-014、REQ-015）

Rust：

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDocument {
    pub content: String,
    pub revision: String,
}

pub fn read_note(root, path) -> AppResult<NoteDocument>;
pub fn write_note(root, path, content, expected_revision: String) -> AppResult<String>;
```

- revision：对文件原始字节做 FNV-1a 64 位哈希，输出 16 位小写十六进制。只用于变更检测，不作为安全校验。
- `read_note` 读取字节后同时返回解码文本与 revision；无效 UTF-8 继续拒绝。
- `write_note` 先读取当前文件字节并计算 revision：
  - 与 `expected_revision` 不同 → `conflict: note was modified outside MD Studio`；
  - 文件缺失 → `conflict: note no longer exists`；
  - 一致 → 写入，返回新内容的 revision。
- 检查与写入之间仍存在极小的 TOCTOU 窗口；不引入跨平台文件锁（ADR-005）。

### 4.2 前端 revision 管理（REQ-014、REQ-015）

- `OpenedNote` 不再保存 revision；使用 `revisionRef` 保存当前文件的 revision，避免 saver 闭包拿到过期值。
- 打开笔记：`read` 返回 `{ content, revision }`，设置 ref 后用内容创建 saver。
- saver 保存：调用 `save(root, path, text, revisionRef.current)`；成功后用返回值更新 `revisionRef` 和 `savedContent`。
- 改名：文件内容未变，revision 不变；重建 saver 时继续使用同一个 ref。
- 冲突错误：保存回调捕获以 `conflict` 开头的错误，设置 `conflict` 状态与 `saveState='conflict'`，不再调度新的自动保存。

### 4.3 冲突 UI（REQ-016）

冲突横幅（编辑器上方）：

```text
磁盘文件已被外部修改。本地内容尚未覆盖磁盘。
[重新载入]  [覆盖保存]
```

- **重新载入**：`saver.cancel()` → `read` 磁盘版本 → 用新内容/revision 替换当前稿 → 重建 saver → 清除冲突。会丢弃本地未保存内容，按钮文案需明确。
- **覆盖保存**：`read` 获取最新 revision（不替换本地内容）→ `save(localContent, latestRevision)` → 取消旧计时器、更新 savedContent/revision → 清除冲突。
- 冲突期间：`handleChange` 继续更新内存内容，但不 `schedule`；切换笔记、打开工作区、关闭窗口都会因 `flush()` 失败而中止。
- `StatusBar` 增加 `conflict` 状态（「磁盘已变更」）。

### 4.4 关闭前 flush（REQ-013）

在 `App` 中注册：

```ts
const appWindow = getCurrentWindow()
await appWindow.onCloseRequested(async (event) => {
  if (closingRef.current) return       // 第二次事件放行
  event.preventDefault()
  if (!(await flushCurrent())) return   // 保存失败/冲突：保持打开
  closingRef.current = true
  try { await appWindow.close() } catch (error) { closingRef.current = false; showError(error) }
})
```

- `flushCurrent()` 等待 `DebouncedSaver.flush()` 完成；失败时保留窗口与 pending 文本。
- 关闭流程中冲突不会被静默覆盖；用户必须先处理横幅。
- 监听器在组件卸载时调用 unlisten，避免重复注册。
- 需要窗口关闭权限（`core:window:allow-close`）。

## 5. 错误处理与可观测性

| 失败模式 | 处理 |
|---|---|
| 外部修改后保存 | Rust 返回 `conflict:`；UI 显示横幅，停止自动保存 |
| 外部删除后保存 | `conflict: note no longer exists`；横幅提示，重新载入会报错 |
| 关闭时保存失败 | 不关闭窗口，错误条保留；用户可重试或处理冲突 |
| 覆盖保存时磁盘又被改 | 第二次 `read` 的 revision 仍可能过期 → 再次 conflict，重复提示 |
| 重新载入读取失败 | 保持冲突状态，错误条显示原因 |
| 关闭事件重入 | `closingRef` 放行第二次事件，避免死循环 |

不引入遥测；错误文本与状态栏是唯一可观测面。

## 6. 验收

| REQ | 验收结果 |
|---|---|
| REQ-013 | **通过 2026-10-23**：`App.test.tsx` 的「flushes pending edits before the window closes」验证 close-request 先 `write_note` 再 `close()`；冲突时「keeps the window open…」验证 `preventDefault` 后不关闭窗口 |
| REQ-014 | **通过 2026-10-23**：Rust `read_and_write_roundtrip`、`revision_is_stable_and_changes_with_content`；`NoteDocument` 返回 16 位 revision |
| REQ-015 | **通过 2026-10-23**：Rust `write_note_detects_conflict_after_external_modification`、`write_note_conflicts_when_note_is_missing`；外部编辑后磁盘保持原文 |
| REQ-016 | **通过 2026-10-23**：`App.test.tsx` 冲突横幅 +「覆盖保存」写入本地文本 +「重新载入」替换为磁盘版本；状态栏显示「磁盘已变更」 |

## 7. 非目标

- 后台轮询/窗口 focus 时自动检测：当前只在保存时检测，避免频繁读盘。
- 三方合并（merge）：冲突只提供整稿重载或整稿覆盖。
- 跨平台文件锁：TOCTOU 窗口保留并在 ADR-005 记录。
- 多窗口/多实例协同：单窗口假设不变。
- 冲突版本另存为副本：留待有需求时扩展。

## 8. 追溯

- 实现：`src-tauri/src/notes.rs`（`revision_for_bytes`、`NoteDocument`、条件写入）、`src/ui/App.tsx`（`revisionRef`、冲突横幅、close flush）、`src/ui/StatusBar.tsx`、`src-tauri/capabilities/default.json`（`core:window:allow-close`）。
- 测试：前端 28 passed（新增 5 个 close/conflict 用例）；Rust 15 passed（新增 3 个 revision/conflict 用例）。
- 命令：`bun run typecheck`、`bun run test`、`bun run build`、`cargo test --locked --all-targets`、`node scripts/check-eng-docs.cjs` 全部通过。
- 实现快照：`docs/specs/2026-10-23-close-flush-and-conflicts-design.md`；计划：`docs/plans/2026-10-23-close-flush-and-conflicts-plan.md`。
- 本次未创建提交（用户未要求）。
