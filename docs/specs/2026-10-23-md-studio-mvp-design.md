# MD Studio MVP 设计快照

> 索引：`RFC-001` | 关联 REQ：REQ-001~REQ-007 | 收益：交付可安装的本地 Markdown 写作闭环
> 位置：本文档为实现快照；权威见 `docs/rfc/RFC-001-md-studio-mvp.md`

## 1. 需求

见 RFC-001 §1。实现范围固定为：工作区选择、递归文件树、打开/编辑/保存、新建去重、GFM 预览清理、路径安全、Windows 安装包。不扩展同步、标签、插件。

## 2. 现状根因

- `src-tauri/src/lib.rs:2` 只有 `greet`，没有文件系统命令。
- `src/App.tsx:1` 是脚手架演示页，无工作区/笔记状态。
- `src-tauri/tauri.conf.json:1` 仍是默认窗口标题与尺寸，未配置 dialog 能力。
- 无测试、无文档门禁；这会使后续演进无法验证。

## 3. 总体设计

指向 RFC-001 §3 的容器图与三条边界，不复制内容。实现上新增：

```text
src/
  core/{notes,tree,autosave}.ts
  application/{ports,workspace}.ts
  adapters/tauri/{noteRepository,picker}.ts
  adapters/markdown/render.ts
  ui/{App,Sidebar,EditorPane,PreviewPane,StatusBar}.tsx
src-tauri/src/{lib,notes}.rs
```

## 4. 详细设计（实现视角增量）

### 4.1 Rust 命令契约

```ts
list_notes(root: string) -> NoteEntry[]
read_note(root: string, path: string) -> string
write_note(root: string, path: string, content: string) -> void
create_note(root: string, title: string) -> NoteEntry
```

- `NoteEntry.path` 是相对路径，统一 `/`；`modifiedMs` 为 Unix 毫秒。
- 命令错误为中文/英文混合的可读字符串？**统一英文小写错误码式消息**，UI 转成中文提示。
- 读取上限 5 MiB；写入内容也检查同一上限。
- 扫描忽略目录名以 `.` 开头，以及 `node_modules`、`target`、`dist`。
- `create_note` 用标题生成文件名：去掉 `.md`、替换 `[\\/:*?"<>|]` 与控制字符为 `-`、压缩连续 `-`、去首尾空格与 `-`；空标题回退 `untitled`；重名追加 `-2`。

### 4.2 核心域接口

```ts
export interface NoteEntry { path; name; title; modifiedMs: number | null }
export function filterNotes(notes: NoteEntry[], query: string): NoteEntry[]
export function buildNoteTree(notes: NoteEntry[]): TreeNode[]
export class DebouncedSaver { schedule(text: string): void; flush(): Promise<void>; cancel(): void }
export function countWords(text: string): number
```

- 字数：CJK 字符逐字计，拉丁/数字按词计。
- 排序：`localeCompare` + 小写兜底；目录优先。
- `DebouncedSaver.flush()` 必须幂等；无待写内容时立即 resolve。
- 保存并发：后一次 `schedule` 覆盖前一次；正在保存时的新内容由下一次计时器处理。

### 4.3 UI 会话

- `App` 在挂载时读取 `mdstudio.workspace`；若 `list_notes` 失败则清空键并显示空态。
- 切换笔记前 `await saver.flush()`；若 flush 失败则中止切换，保留当前稿。
- 新建笔记后刷新列表并打开新文件。
- 视图模式：`split | edit | preview`，桌面宽度 < 860px 时强制单栏（CSS）。
- 错误条显示最近一次错误，可手动关闭；保存失败时 dirty 保持。

### 4.4 预览安全

- `marked` 开启 `gfm: true`、`breaks: true`。
- `DOMPurify.sanitize(html, { USE_PROFILES: { html: true } })`。
- 外链点击 `preventDefault`，仅 `http/https` 交给 `openUrl`；`#` 锚点放行。

### 4.5 构建

- `src-tauri/rust-toolchain.toml` 固定 MSVC。
- `tauri.conf.json`：productName `MD Studio`，窗口 1180×780、最小 900×600，`bundle.targets: ["nsis"]`。
- 能力文件增加 `dialog:default`；移除未使用的 `opener:default`？保留，外链需要。

## 5. Self-Review

- 数据语义：保存为 UTF-8 原文，不改换行；读取拒绝非 UTF-8，避免静默损坏。
- 与既有机制共存：与 Typora/VS Code 同时编辑时采用最后写入获胜；风险入 backlog B1-4。
- 回归：`bun run test`、`bun run typecheck`、`bun run build`、`cargo test`、`bun run tauri build` 全部通过；文档门禁 PASS。
