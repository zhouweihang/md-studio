# MD Studio MVP 实现计划（2026-10-23）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付一个可安装的 Tauri 桌面 Markdown 笔记应用：选择本地文件夹、浏览/搜索、编辑、GFM 预览、700ms 自动保存，并有可重复的测试与文档门禁。

**Status:** MVP 已实现并验证（前端 21 passed、Rust 7 passed、typecheck、前端构建、NSIS 1.50 MiB、文档门禁 PASS）。执行本计划时目录尚未初始化 Git，因此各 Task 的「提交」步骤保持未勾选：2026-10-23 初始化仓库后，整棵树随首批提交入库，没有按 Task 拆分提交。

---

## File Structure
- Create: `src/core/{notes,tree,autosave}.ts` 与对应 `*.test.ts`
- Create: `src/application/{ports,workspace}.ts` 与 `workspace.test.ts`
- Create: `src/adapters/tauri/{noteRepository,picker}.ts`
- Create: `src/adapters/markdown/render.ts` 与 `render.test.ts`
- Create: `src/ui/{App,Sidebar,EditorPane,PreviewPane,StatusBar}.tsx`、`src/ui/app.css`
- Create: `src-tauri/src/notes.rs`；Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/rust-toolchain.toml`
- Modify: `package.json`、`vite.config.ts`、`tsconfig.json`、`index.html`、`src/main.tsx`
- Modify: `src-tauri/{Cargo.toml,tauri.conf.json,capabilities/default.json}`
- Create: `README.md`、`AGENTS.md`、`scripts/check-eng-docs.cjs`、`eng-docs.config.json`
- Modify: `docs/backlog.md`、`docs/rfc/RFC-001-md-studio-mvp.md` §6/§8（收尾）

---

### Task 0: 工具链与脚手架
**Files:** `package.json`、`src-tauri/*`、`rust-toolchain.toml`

- [x] **Step 1**: 用 `create-tauri-app` 生成 Tauri 2 + React + TS 脚手架。
- [x] **Step 2**: 确认 Rust MSVC、WebView2、Node 24、bun 1.3 可用。
- [x] **Step 3**: 添加 `@tauri-apps/plugin-dialog`、`marked`、`dompurify`、`vitest`、`jsdom`；补 `test`/`typecheck` 脚本。
- [ ] **Step 4**: 提交 — `chore(setup): scaffold tauri app and test tooling`

### Task 1: Rust 文件系统边界
**Files:** `src-tauri/src/notes.rs`、`src-tauri/src/lib.rs`、`src-tauri/Cargo.toml`

- [x] **Step 1**: 写失败测试：递归列表、跳过隐藏、读写往返、重名去重、拒绝 `..`/绝对路径/非 md。
- [x] **Step 2**: 实现 `NoteEntry`、`canonical_root`、`relative_note_path`、`list_notes`、`read_note`、`write_note`、`create_note`。
- [x] **Step 3**: 在 `lib.rs` 注册命令并启用 dialog 插件。
- [x] **Step 4**: `cargo test` 全绿（MSVC 工具链）。
- [ ] **Step 5**: 提交 — `feat(fs): add safe workspace note commands`

### Task 2: 核心域
**Files:** `src/core/{notes,tree,autosave}.ts` 与测试

- [x] **Step 1**: 先写测试：CJK/拉丁字数、路径过滤、目录优先树、防抖与 flush、取消。
- [x] **Step 2**: 实现纯函数与 `DebouncedSaver`；core 不 import React/Tauri。
- [x] **Step 3**: `bun run test` 全绿（core 14 个测试）。
- [ ] **Step 4**: 提交 — `feat(core): add note tree, filtering and debounced autosave`

### Task 3: 应用层与适配器
**Files:** `src/application/*`、`src/adapters/*` 与测试

- [x] **Step 1**: 定义 `NoteRepository`/`WorkspacePicker` 端口与 `WorkspaceService`。
- [x] **Step 2**: 用内存仓库测试 load/read/save/create 的委托行为与错误透传。
- [x] **Step 3**: 实现 `TauriNoteRepository`、`pickWorkspace`、`renderMarkdown`；测试 XSS 清理与 GFM 表格。
- [ ] **Step 4**: 提交 — `feat(app): wire workspace service and tauri adapters`

### Task 4: 渲染层
**Files:** `src/ui/*`、`src/App.tsx`、`src/main.tsx`、`index.html`、CSS

- [x] **Step 1**: 空态页 + 打开文件夹 + 恢复上次工作区。
- [x] **Step 2**: 侧栏搜索、目录树、新建笔记、错误条。
- [x] **Step 3**: 编辑器、分栏/编辑/预览模式、Ctrl+S、状态栏、外链拦截。
- [x] **Step 4**: `bun run typecheck` 与 `bun run build` 通过。
- [ ] **Step 5**: 提交 — `feat(ui): add workspace sidebar, editor and preview`

### Task 5: 全量验证
**Files:** `package.json`、`vite.config.ts`

- [x] **Step 1**: `bun run test` —— 6 files / 21 passed。
- [x] **Step 2**: `cargo test` —— 7 passed。
- [x] **Step 3**: `bun run typecheck`、`bun run build` 通过。
- [ ] **Step 4**: 提交 — `test: verify mvp behavior across rust and web`

### Task 6: 桌面打包与收尾
**Files:** `src-tauri/tauri.conf.json`、`README.md`、`AGENTS.md`、`docs/*`

- [x] **Step 1**: 配置产品名、窗口、NSIS，执行 `bun run tauri build`，确认安装包存在（1.50 MiB）。
- [x] **Step 2**: 跑文档门禁 `node scripts/check-eng-docs.cjs`（PASS）。
- [x] **Step 3**: 回填 RFC-001 §6/§8、更新 backlog、C3 追溯。
- [ ] **Step 4**: 提交 — `docs: record mvp verification and packaging`

---

## Self-Review
- 数据语义：只处理 UTF-8 文本；不改变换行与 Markdown 原文；超限拒绝而非截断。
- 与既有机制共存：外部编辑采用最后写入获胜；无数据库/缓存，不产生陈旧索引。
- 回归：Web 测试、Rust 测试、类型检查、前端构建、Tauri 打包、文档门禁六项全部通过才算完成。
