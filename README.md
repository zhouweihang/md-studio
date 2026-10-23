# MD Studio

本地优先的 Markdown 笔记桌面应用。选择一个文件夹，浏览其中的 Markdown 文件，在源码与实时预览之间写作，修改会自动保存回磁盘。

技术栈：**Tauri 2 · Rust · React 19 · TypeScript · Vite 8 · bun**。

## 功能（v0.1.0）

- 选择任意本地文件夹作为工作区，递归发现 `.md/.markdown/.mdown`。
- 目录树（目录优先、名称排序）与按文件名/路径搜索。
- 源码编辑 + GFM 实时预览（表格、任务列表、代码块），危险 HTML 经 DOMPurify 清理。
- 700ms 防抖自动保存；`Ctrl+S` 立即保存；切换笔记前强制落盘。
- 新建笔记，非法字符清洗，重名自动追加 `-2`、`-3`。
- 记住上次工作区；保存状态、字数、行数、相对路径可见。
- Rust 侧统一校验路径：拒绝 `..`、绝对路径、非 Markdown 扩展名与符号链接逃逸。

## 快速开始

环境要求：

- Node.js 24+ 与 [bun](https://bun.sh/)
- Rust stable **MSVC** 工具链（项目根 `rust-toolchain.toml` 已固定）
- Windows：WebView2 与 Visual Studio Build Tools（C++ 工作负载）

```bash
bun install
bun run tauri dev      # 开发模式（Vite + Tauri）
bun run tauri build    # 生成 NSIS 安装包
```

常用检查：

```bash
bun run test           # Vitest：core / application / markdown 适配器
bun run typecheck      # tsc --noEmit
bun run build          # 前端生产构建
bun run check:docs     # RFC / ADR / C4 / spec / plan / backlog 门禁
cd src-tauri && cargo test
```

## 持续集成

`.github/workflows/ci.yml` 在 push 到 `main`、Pull Request 和手动触发时运行三个独立 job：

- **web**：`bun install --frozen-lockfile` → `bun run test` → `bun run typecheck` → `bun run build`
- **rust**：Windows MSVC 工具链，先构建前端供 Tauri codegen 使用，再执行 `cargo test --locked --all-targets`
- **docs**：`node scripts/check-eng-docs.cjs`，检查 RFC / ADR / C4 / spec / plan / backlog 的门禁

## 架构

依赖方向永远指向内层：

```text
src/ui             React 组件与会话状态
src/adapters       Tauri invoke、目录选择、marked + DOMPurify
src/application    WorkspaceService + NoteRepository 端口
src/core           纯领域规则：树、过滤、防抖、字数
src-tauri/src      Rust 文件系统边界 + 路径校验
```

- `src/core` 不 import React、DOM 或 Tauri，可直接在 Node 中单测。
- 前端只有 `src/adapters/tauri/noteRepository.ts` 知道命令名；UI 不直接 `invoke`。
- 文件系统是唯一事实源，不建数据库、不缓存正文；`localStorage` 只保存工作区路径。
- 所有笔记操作使用「工作区根 + 相对路径」，Rust 再次 canonicalize 并检查 `starts_with(root)`。

更多设计：

- 权威设计：[`docs/rfc/RFC-001-md-studio-mvp.md`](docs/rfc/RFC-001-md-studio-mvp.md)
- 决策记录：[`docs/adr/README.md`](docs/adr/README.md)
- 结构现状：[`docs/architecture/C1-context.md`](docs/architecture/C1-context.md)、[`C2`](docs/architecture/C2-container.md)、[`C3`](docs/architecture/C3-component.md)
- 实现快照：[`docs/specs/2026-10-23-md-studio-mvp-design.md`](docs/specs/2026-10-23-md-studio-mvp-design.md)
- 执行计划：[`docs/plans/2026-10-23-md-studio-mvp-plan.md`](docs/plans/2026-10-23-md-studio-mvp-plan.md)
- 遗留总账：[`docs/backlog.md`](docs/backlog.md)

## 安全模型

- WebView 不启用 `fs` 插件，不持有任意路径读写能力。
- 所有路径在 Rust 侧校验：只允许工作区内的相对 Markdown 路径；符号链接目录被跳过。
- 单文件上限 5 MiB；工作区最多扫描 10,000 篇，目录深度上限 12。
- 预览 HTML 先经 DOMPurify 清理；外链由系统浏览器打开，不在 WebView 内导航。

## 已知限制

- 没有删除/重命名、全文搜索、文件监听、多标签。
- 外部编辑器并发修改时采用最后写入获胜，没有冲突检测。
- 关闭窗口时未强制 flush（切换笔记与 Ctrl+S 已覆盖）。
- 当前只验证 Windows；CI 跨平台矩阵尚未建立。

完整清单与收尾标准见 [`docs/backlog.md`](docs/backlog.md)。
