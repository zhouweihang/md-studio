# ADR-001: 用 Tauri 2 作为桌面外壳

> Status: Accepted | Date: 2026-10-23 | Deciders: 项目所有者 | 关联 RFC: RFC-001 §3 | 影响：构建链、安装包体积、Rust/前端边界

## Context

MD Studio 需要直接读写用户选择的本地 Markdown 文件夹，并交付一个可安装的 Windows 桌面程序。仓库已有 Node 24、bun 1.3、Rust 1.96 与 MSVC BuildTools，`create-tauri-app` 已生成 Tauri 2 + React 19 + Vite 8 脚手架（`src-tauri/Cargo.toml:1`、`package.json:1`）。

约束：

- 前端团队已熟悉 React/TypeScript，希望保留 Web 技术栈。
- 文件系统访问必须收敛在一个可审计的边界内。
- 安装包应尽量小，冷启动不应依赖本地服务器。
- 不引入需要单独运行时或长期维护原生模块的方案。

## Decision

1. 采用 **Tauri 2** 作为桌面外壳，前端为 **React 19 + TypeScript + Vite 8**，包管理用 bun。
2. 文件系统能力用 **Rust 命令**实现（`list_notes` / `read_note` / `write_note` / `create_note`），不启用 `fs` 插件，避免把任意路径读写权限暴露给 WebView。
3. 文件夹选择使用官方 `tauri-plugin-dialog`，仅授予 `dialog:default` 能力；外链打开复用已存在的 `opener` 插件。
4. Windows 构建固定使用 **MSVC** 工具链：在项目根 `rust-toolchain.toml` 声明 `stable-x86_64-pc-windows-msvc`，避免机器默认的 GNU 工具链与 WebView2 组合出不一致产物。
5. 打包目标为 NSIS（`bundle.targets` 含 `nsis`），产出单个 `.exe` 安装包。

## Alternatives

- **Electron**：生态成熟，但安装包与内存开销明显更大，且需要维护 Chromium 版本；对本项目的轻量写作场景不划算。
- **纯浏览器 PWA**：无法可靠地递归读写用户任意文件夹（File System Access API 兼容性与权限模型受限），不满足「本地文件夹即工作区」。
- **Rust 原生 GUI（egui/Slint）**：文件系统边界最直接，但放弃 React 生态与 Markdown 渲染库，UI 迭代成本高。
- **Tauri + `fs` 插件**：配置更少，但需要给 WebView 开放 scope；路径校验分散在 JS 配置与运行时，审计面更大。

## Consequences

- 正：安装包体积小、内存占用低；Rust 侧可对路径做统一校验；前端仍可用 React 生态。
- 正：命令契约稳定后，未来替换 UI 或增加 CLI 复用同一套 Rust 逻辑。
- 负：团队需要维护 Rust 与前端两套工具链；首次编译明显慢于纯 Web。
- 负：WebView2 在旧版 Windows 上可能需要额外安装（Tauri 可引导安装）。
- 风险：Rust 命令与前端类型可能漂移。缓解：命令参数/返回结构集中定义，前端只通过 `adapters/tauri` 调用，并用集成测试覆盖关键路径。

## References

- `md-studio/src-tauri/Cargo.toml:1`（Tauri 2 依赖）
- `md-studio/src-tauri/tauri.conf.json:1`（窗口与打包配置）
- `md-studio/src-tauri/capabilities/default.json:1`（能力白名单）
- RFC-001 §3、§4.1；ADR-002；ADR-003
