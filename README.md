# MD Studio

本地优先的 Markdown 笔记桌面应用。选择一个文件夹，浏览其中的 Markdown 文件，在源码与实时预览之间写作，修改会自动保存回磁盘。

技术栈：**Tauri 2 · Rust · React 19 · TypeScript · Vite 8 · bun**。

## 功能（v0.1.0）

- 选择任意本地文件夹作为工作区，递归发现 `.md/.markdown/.mdown`。
- 目录树（目录优先、名称排序、默认折叠）与按文件名/路径搜索；侧栏按固定行高虚拟化，50k 工作区也只渲染视口内的约 50 行，支持方向键、Enter/Space 导航。
- 全文搜索正文（大小写不敏感、行号、上下文片段），命中词在片段中高亮，点击直接定位到行。
- 文件监听把外部编辑器（Typora / VS Code / Git）的改动自动同步进搜索快照；外部新建、删除与改名也会直接更新侧栏文件树，无需手动刷新。
- 重命名当前笔记（保留目录与扩展名，冲突拒绝）；删除移入系统回收站，并可立即撤销。
- 源码编辑 + GFM 实时预览（表格、任务列表、代码块），危险 HTML 经 DOMPurify 清理。
- 700ms 防抖自动保存；`Ctrl+S` 立即保存；切换笔记、切换工作区、关闭窗口前强制落盘。
- 保存时比较内容 revision：文件被外部编辑器改过则拒绝覆盖，提供「重新载入」或「覆盖保存」。
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
bun run check:perf     # 最新基准结果对比基线：中位数 ≤1.2×、p95 ≤2×
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
- 文件系统是唯一事实源，不建索引或持久化正文库；正文只在进程内做只读缓存（ADR-007），退出即丢弃。`localStorage` 只保存工作区路径。
- 所有笔记操作使用「工作区根 + 相对路径」，Rust 再次 canonicalize 并检查 `starts_with(root)`。

更多设计：

- 权威设计：[`docs/rfc/RFC-001-md-studio-mvp.md`](docs/rfc/RFC-001-md-studio-mvp.md)、[`RFC-002`](docs/rfc/RFC-002-note-management-and-search.md)、[`RFC-003`](docs/rfc/RFC-003-close-flush-and-conflicts.md)、[`RFC-004`](docs/rfc/RFC-004-trash-and-undo.md)、[`RFC-005`](docs/rfc/RFC-005-search-highlight-and-perf.md)、[`RFC-006`](docs/rfc/RFC-006-search-performance.md)
- 搜索决策：[`ADR-004`](docs/adr/004-rust-scan-search.md)（Superseded by [`ADR-007`](docs/adr/007-search-snapshot-cache.md)）、[`ADR-008`](docs/adr/008-streaming-search-channel.md)、[`ADR-009`](docs/adr/009-workspace-watcher.md)
- 并发决策：[`ADR-005`](docs/adr/005-content-revision-concurrency.md)
- 删除决策：[`ADR-006`](docs/adr/006-system-trash-and-snapshot-undo.md)
- 决策记录：[`docs/adr/README.md`](docs/adr/README.md)
- 结构现状：[`docs/architecture/C1-context.md`](docs/architecture/C1-context.md)、[`C2`](docs/architecture/C2-container.md)、[`C3`](docs/architecture/C3-component.md)
- 实现快照：[`docs/specs/2026-10-23-md-studio-mvp-design.md`](docs/specs/2026-10-23-md-studio-mvp-design.md)
- 执行计划：[`docs/plans/2026-10-23-md-studio-mvp-plan.md`](docs/plans/2026-10-23-md-studio-mvp-plan.md)
- 遗留总账：[`docs/backlog.md`](docs/backlog.md)

## 安全模型

- WebView 不启用 `fs` 插件，不持有任意路径读写能力。
- 所有路径在 Rust 侧校验：只允许工作区内的相对 Markdown 路径；符号链接目录被跳过。
- 单文件上限 5 MiB；工作区最多扫描 50,000 篇，目录深度上限 12。
- 预览 HTML 先经 DOMPurify 清理；外链由系统浏览器打开，不在 WebView 内导航。

## 已知限制

- 全文搜索优先读打开工作区时建立的内存快照（默认预算 256 MB，可用 `MDSTUDIO_SEARCH_CACHE_MB` 覆盖），未就绪/被淘汰/超限时回退实时扫描；结果经 Channel 每批 ≤16 条流式返回，`Done` 汇总总数、耗时与来源，新查询会取消旧任务；最多 50 条、每篇最多 3 条。
- 没有多标签；外部改动由文件监听进入搜索快照，冲突仍只在保存时检测，不做后台轮询或三方合并。
- 冲突检查与写入之间仍有极小的 TOCTOU 窗口，不引入跨平台文件锁（ADR-005）。
- 撤销只覆盖最近一次删除且仅在当前会话；超过 5 MiB 或非 UTF-8 的文件仍可删除，但没有应用内撤销。
- 恢复只写回内容与路径，不保留文件元数据；更早的删除请到系统回收站找回。
- 50k 工作区打开成本（本机 release，1,086 MiB 语料）：枚举 + stat 1,029 ms，IPC JSON 5.10 MB，`buildNoteTree` 149 ms；随后后台预热。侧栏 DOM 不随笔记数增长。
- 搜索性能（本机 release，语料 27/216/1,086 MiB，2026-10-25 流式路径）：暖 p50 3.7 / 14.3 / 1,999.8 ms，p95 4.7 / 17.5 / 2,034.8 ms；10k 档首批结果 0.65 ms（p50，常见词 50 条命中）。50k 受 256 MB 预算限制，只缓存约 11.5k 篇，其余每查询回退读盘。`--no-snapshot` 可测磁盘路径。
- 文件监听（RFC-006 Task 6 / ADR-009）：`notify` 递归监听工作区，300 ms 去抖后增量更新快照与文件树（新增/删除/改名），外部改动实测 300–400 ms 可见，正文修改不会触发树刷新。监听不可用或事件溢出时置 `Degraded`，此后每次查询前按 mtime+size 校验缓存正文：10k 档降级查询 p50 183 ms（校验约 169 ms），仍快于完整磁盘扫描的 755 ms，但明显慢于就绪态的 14.6 ms。状态栏常驻显示快照三态（预热中 / 搜索缓存 / 已降级）与缓存大小。`bench_search --degraded` 可复现该档位。
- 当前只验证 Windows；CI 跨平台矩阵尚未建立。

完整清单与收尾标准见 [`docs/backlog.md`](docs/backlog.md)。
