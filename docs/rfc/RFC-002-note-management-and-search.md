# RFC-002: 笔记删除、重命名与全文搜索（域 Notes）

> Status: Implemented (Completed 2026-10-23) | Type: RFC | Created: 2026-10-23 | Updated: 2026-10-23 | Authors: Buffy | Reviewers: 项目所有者 | Target: v0.2.0
> 索引：`RFC-002` | 关联 REQ：REQ-008~REQ-012 | 收益：把「找到一篇笔记」和「整理一篇笔记」从外部文件管理器收回应用内，搜索不再只看文件名。
> 位置：`docs/rfc/RFC-002-note-management-and-search.md` 权威；`docs/specs/2026-10-23-note-management-and-search-design.md` 为实现快照
> 一致性声明：本文档与 RFC-001（MVP 基线）、ADR-003（文件系统是唯一事实源）、ADR-004（Rust 扫描搜索）保持一致；不改变 core/application/adapters/ui 的依赖方向。

## 0. 更新摘要

2026-10-23：建立 RFC-002。RFC-001 将删除/重命名与全文搜索列为非目标；本 RFC 在现有文件系统事实上补齐这三项能力。删除采用「确认后永久删除」，不引入回收站；重命名只改同一目录下的文件名；全文搜索由 Rust 扫描 Markdown 正文，不建持久索引。

2026-10-23：实现完成。前端测试 27 passed（新增 6 个 UI/对话框测试），Rust 12 passed（新增 5 个删除/改名/搜索测试）；typecheck、build 与文档门禁通过。

## 1. 需求

| REQ | 能力 | 基线 | 目标 | 验证 |
|---|---|---|---|---|
| REQ-008 | 删除当前笔记 | 0：`NoteRepository` 只有 list/read/write/create（`src/application/ports.ts:3`） | 二次确认后从磁盘删除；取消不产生副作用；路径越界拒绝 | `cd md-studio/src-tauri && cargo test delete_note` |
| REQ-009 | 重命名当前笔记 | 0：无 rename 命令，只能退出应用手动改名 | 同目录改名，保留扩展名；非法字符清洗；目标已存在时拒绝；改名成功后继续编辑并保存到新路径 | `cd md-studio/src-tauri && cargo test rename_note` |
| REQ-010 | 全文搜索 | 0：侧栏只按文件名/路径过滤（`src/ui/Sidebar.tsx:99`） | 大小写不敏感匹配正文，返回相对路径、行号、上下文片段；最多 50 条、每篇最多 3 条 | `cd md-studio/src-tauri && cargo test search_notes` |
| REQ-011 | 搜索与整理 UI | 0：无删除/重命名入口，无内容搜索结果 | 工具栏提供重命名/删除；搜索框 250ms 防抖显示内容命中，点击命中打开对应笔记并定位到行 | `cd md-studio && bun run test App` |
| REQ-012 | 验证与文档 | 0：无相关测试 | Rust 单测覆盖删除/改名/搜索；UI 测试覆盖搜索命中与改名/删除流程；文档门禁 PASS | `cd md-studio && bun run test && node scripts/check-eng-docs.cjs` |

## 2. 现状根因

- `src/application/ports.ts:3` 的 `NoteRepository` 只有 `listNotes/readNote/writeNote/createNote`，应用层无法表达删除、改名、正文搜索。
- `src-tauri/src/notes.rs` 只有 `list_notes/read_note/write_note/create_note`，前端没有可调用的删除/改名/搜索命令。
- `src/ui/Sidebar.tsx:99` 的搜索只调用 `filterNotes`，匹配 `path/title`，不读取正文；用户无法按内容定位。
- `src/ui/App.tsx` 的工具栏只有视图切换，没有笔记级操作；也没有可复用的确认/重命名对话框。
- 自动保存使用「打开时的路径」闭包（`src/ui/App.tsx` 中 `pathAtOpen`），改名后必须重建 saver，否则会继续写旧路径。

## 3. 总体设计

```mermaid
flowchart LR
  UI[Sidebar / NoteDialog] --> APP[App 会话状态]
  APP --> SVC[WorkspaceService]
  SVC --> PORT[NoteRepository]
  PORT --> ADAPTER[TauriNoteRepository]
  ADAPTER -->|delete_note / rename_note / search_notes| RUST[src-tauri/src/notes.rs]
  RUST --> FS[(Markdown 文件)]
  RUST -->|SearchHit[]| ADAPTER
```

设计原则不变：

1. 文件系统仍是唯一事实源；删除是 `remove_file`，改名是 `rename`，搜索每次实时读盘。
2. UI 不直接 `invoke`；新增能力先扩展端口与应用服务。
3. Rust 是唯一路径校验点；删除/改名/搜索都复用 `canonical_root`、`relative_note_path`、`resolve_existing`。

## 4. 详细设计

### 4.1 删除（REQ-008）

Rust：

```rust
#[tauri::command]
pub fn delete_note(root: String, path: String) -> AppResult<()> {
    let root = canonical_root(&root)?;
    let target = resolve_existing(&root, &path)?;
    fs::remove_file(&target).map_err(|error| format!("cannot delete note: {error}"))
}
```

UI 流程：

1. 工具栏「删除」打开确认对话框，显示文件名与「不可恢复」提示。
2. 确认后先 `saver.cancel()` 丢弃待写定时器，再调用 `delete_note`；避免删除后定时器又把文件写回来。
3. 成功：刷新列表、关闭当前笔记、清空 saver；失败：保留当前笔记并显示错误。
4. 不做回收站（见 §7）；该限制记录在 backlog B1-7。

### 4.2 重命名（REQ-009）

Rust：

```rust
#[tauri::command]
pub fn rename_note(root: String, path: String, new_title: String) -> AppResult<NoteEntry>
```

规则：

- 源路径经 `resolve_existing` 校验；目标目录固定为源文件所在目录，扩展名沿用源文件（`md/markdown/mdown`）。
- 新名称经 `sanitize_title` 清洗（去掉 Markdown 扩展名、非法字符替换、空白折叠、Windows 保留名处理）。
- 目标已存在且不是源文件本身 → 返回 `a note with that name already exists`，不覆盖。
- 成功后返回新的 `NoteEntry`；内容不复制、不重写。

UI 流程：

1. 工具栏「重命名」打开对话框，预填当前文件 stem。
2. 提交前 `await saver.flush()`；失败则中止，保留旧文件与未保存内容。
3. 调用 `rename_note`，刷新列表后用返回的 `NoteEntry` 重新打开笔记（`openNote`），从而用新路径重建 saver。
4. 命名冲突留在对话框内提示，不关闭。

### 4.3 全文搜索（REQ-010）

Rust 侧新增异步命令：

```rust
#[tauri::command]
pub async fn search_notes(root: String, query: String) -> AppResult<Vec<SearchHit>>
```

- 空白查询返回空数组；否则用 `list_notes` 的扫描结果逐篇读取 UTF-8 正文。
- 逐行 `to_lowercase().contains(query.to_lowercase())`，大小写不敏感；行号从 1 开始。
- 每篇最多 3 条，总计最多 50 条；命中后生成最多约 90 字符的上下文片段（Unicode 安全截断，首尾加 `…`）。
- 读取失败或非 UTF-8 的文件跳过，不阻断其他结果。
- 通过 `tauri::async_runtime::spawn_blocking` 执行扫描，避免阻塞 UI 线程；不建索引（ADR-004）。

UI：

1. 查询非空时 250ms 防抖调用 `search_notes`；查询变化用取消标记丢弃旧响应。
2. 侧栏在文件名匹配树之外增加「内容匹配」分组，展示路径、行号、片段；无结果时显示空态。
3. 点击命中项通过 `App.openNote(note, line)` 打开，并把编辑器选区定位到该行。
4. 搜索失败只显示错误条，不清空当前打开的笔记。

### 4.4 交互组件（REQ-011）

- 新增 `src/ui/NoteDialog.tsx`：`rename | delete` 两种模式，支持 Escape 取消、Enter 提交、提交中禁用按钮。
- 工具栏增加「重命名」「删除」按钮（仅在有打开笔记时显示）。
- 搜索框继续保留文件名/路径匹配；内容命中与文件树并列展示，避免替换现有习惯。
- 所有异步操作在 `App` 中串行处理：改名/删除前先 flush 或 cancel，避免与自动保存竞态。

## 5. 错误处理与可观测性

| 失败模式 | 处理 |
|---|---|
| 删除时文件已被外部删除 | Rust 返回 `cannot open note`；UI 刷新列表并提示，不崩溃 |
| 改名目标已存在 | Rust 返回冲突消息；对话框保持打开，用户改名重试 |
| 改名时 flush 失败 | 中止改名，保留旧路径与 dirty 状态 |
| 搜索读取单个文件失败 | 跳过该文件，继续扫描；不返回半截错误 |
| 搜索结果过期 | 查询变化时取消标记丢弃旧响应 |
| 查询过长 / 空查询 | 空查询不请求；长查询仍受 50 条上限保护 |

日志：沿用 `console.error` 与错误条，不引入遥测。

## 6. 验收

| REQ | 验收结果 |
|---|---|
| REQ-008 | `delete_note_removes_file_and_rejects_traversal` 通过；`App.test.tsx` 删除流程通过 |
| REQ-009 | `rename_note_preserves_directory_and_extension`、`rename_note_rejects_existing_target` 通过；UI 改名流程通过 |
| REQ-010 | `search_notes_finds_content_with_line_and_preview` 通过（大小写、中文、行号、文件名-only 不命中、空查询） |
| REQ-011 | `App.test.tsx` 5 个流程 + `NoteDialog.test.tsx` 3 个对话框测试通过；搜索命中定位到第 3 行 |
| REQ-012 | 前端 7 files / 27 passed；Rust 12 passed；`bun run typecheck`、`bun run build`、NSIS 1.51 MiB、文档门禁 PASS |

## 7. 非目标

- 回收站/撤销删除：需要平台 API 或自建暂存区，本期用确认对话框降低误删。
- 正则、全词、模糊搜索：先把字面子串搜索做稳。
- 搜索替换/批量删除：高风险，等有明确需求再做。
- 持久化全文索引：当前工作区规模用实时扫描足够（ADR-004 给出切换阈值）。
- 跨目录移动笔记：重命名只改同一目录内的文件名。

## 8. 追溯

实现基于初始提交 `e729d6a`；本次功能改动尚未创建新提交（用户未要求提交）。

- 实现位置：`src-tauri/src/{lib,notes}.rs`、`src/application/{ports,workspace}.ts`、`src/adapters/tauri/noteRepository.ts`、`src/core/search.ts`、`src/ui/{App,Sidebar,EditorPane,NoteDialog}.tsx`
- 新增/修改测试：`src-tauri/src/notes.rs`（12 个单测）、`src/application/workspace.test.ts`、`src/ui/App.test.tsx`（5）、`src/ui/NoteDialog.test.tsx`（3）
- 最终结果：前端 7 files / 27 passed；Rust 12 passed；`bun run typecheck`、`bun run build`、`bun run tauri build`（NSIS 1.51 MiB）、`bun run check:docs` 均成功

> 实现追溯：无新增提交集；验收数据采集于 2026-10-23，环境 Windows x64 + MSVC + WebView2。
