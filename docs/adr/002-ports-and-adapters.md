# ADR-002: 核心域与平台适配器分离

> Status: Accepted | Date: 2026-10-23 | Deciders: 项目所有者 | 关联 RFC: RFC-001 §3、§4 | 影响：目录结构、测试策略、未来替换平台的范围

## Context

项目目标是「学习软件架构并持续维护进化」，因此首版就不能把 `invoke`、DOM、`localStorage` 散落在组件里。脚手架当前只有一个演示组件（`src/App.tsx:1`），没有任何分层约束；如果直接在其中堆功能，后续加入搜索、标签、同步时会产生难以测试的耦合。

约束：

- 领域规则（树构造、过滤、防抖、字数）应能在 Node 中快速单测，不启动 Tauri。
- 平台能力（文件、对话框、外链）必须可替换，未来可能增加浏览器演示版或 CLI。
- React 只负责渲染与事件，不承载业务规则。

## Decision

采用 Ports & Adapters（六边形）分层，依赖方向永远指向内层：

```text
src/ui/            React 组件与 hooks（最外层）
src/adapters/      Tauri invoke、dialog、marked/DOMPurify 实现
src/application/   WorkspaceService + NoteRepository 端口（用例编排）
src/core/          纯 TypeScript 领域规则（无 DOM、无 Tauri、无 React）
src-tauri/src/     Rust 文件系统边界（命令 + 校验 + 单测）
```

1. **`core` 零依赖**：只导出纯函数/纯类，例如 `buildNoteTree`、`filterNotes`、`DebouncedSaver`、`countWords`；测试不 mock 任何平台。
2. **`application` 定义端口**：`NoteRepository` 接口声明 `list/read/write/create`；`WorkspaceService` 只依赖该接口，负责把用例串起来。
3. **`adapters` 实现端口**：`TauriNoteRepository` 把方法映射为 `invoke`；`renderMarkdown` 封装 `marked` + `DOMPurify`；`pickWorkspace` 封装 dialog。
4. **`ui` 只依赖 application/core**：组件通过 props 接收数据，副作用集中在 `App.tsx` 与少量 hooks。
5. **Rust 命令是第二道边界**：即使前端被注入，越界路径也会在 Rust 侧被拒绝（RFC-001 §4.1）。

## Alternatives

- **组件内直接 `invoke`**：初期代码最少，但测试必须 mock Tauri，平台细节渗透到 UI，违背本项目的学习目标。
- **引入 Redux/Zustand 作为状态层**：当前只有单工作区、单笔记，React 本地状态足够；额外状态库会增加概念负担。
- **把领域逻辑也放进 Rust**：性能更好，但树构造/过滤/防抖属于展示逻辑，放 Rust 会导致每次输入跨语言往返，且前端测试仍需重复实现。
- **全量六边形（Rust 侧也分 domain/application/adapters）**：当前 Rust 只做文件系统适配，分层会过度设计；等出现第二个文件系统后端（如 S3）再拆分。

## Consequences

- 正：`core` 与 `application` 可在毫秒级完成单测，不依赖 WebView2 或 Rust 编译。
- 正：未来替换桌面壳（如增加 Web 预览）只需替换 `adapters`。
- 正：Rust 命令契约清晰，前端类型集中在 `NoteEntry`。
- 负：多一层间接调用，简单功能也需要跨 2~3 个文件。
- 负：需要维护前端与 Rust 两侧的错误语义映射（当前统一为字符串）。
- 风险：分层被时间压力打破（例如在组件里直接 `invoke`）。缓解：代码评审以「ui 目录不得 import `@tauri-apps/api/core`」为硬规则，适配器除外。

## References

- RFC-001 §3、§4
- ADR-001（Tauri 外壳）、ADR-003（文件系统事实源）
- `md-studio/src-tauri/src/lib.rs:1`（当前命令注册点）
