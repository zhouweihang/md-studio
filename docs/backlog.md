# Backlog 待办清单（工程级）

> Status: Active | Created: 2026-10-23 | Updated: 2026-10-23 | 用途：归档跨阶段遗留项，防止遗漏；每项含证据、现状、收尾标准
> 关联：RFC-001、ADR-001~003

## B0. 已闭环（P0/P1/P2）

- **2026-10-23 MVP 闭环**：REQ-001~REQ-007 全部验收；前端 21 passed、Rust 7 passed、`typecheck`/`build`/NSIS 1.50 MiB/文档门禁 PASS。
- **2026-10-23 Git/CI 接入**：初始化 `main` 仓库；`.github/workflows/ci.yml` 覆盖 web 测试/类型/构建、Windows MSVC `cargo test`、文档门禁。

## B1. 产品能力遗留（MVP 之外）

| ID | 项 | 证据 | 现状 | 收尾标准 |
|---|---|---|---|---|
| B1-1 | 删除/重命名笔记 | RFC-001 §7 非目标 | 未实现；只能通过外部文件管理器操作 | 侧栏提供删除/重命名，二次确认，Rust 侧同样做路径校验 + 单测 |
| B1-2 | 全文搜索 | RFC-001 §4.4 仅按路径过滤 | 未实现；大工作区无法按内容定位 | 输入 300ms 内返回命中片段；>5000 文件时用派生索引，需新 ADR |
| B1-3 | 文件系统监听 | ADR-003 Decision 2 | 未实现；外部新增文件需手动刷新 | 监听工作区变化并增量更新树；重命名/原子保存不丢事件 |
| B1-4 | 并发编辑冲突检测 | ADR-003 Decision 4 | 未实现；最后写入获胜，可能覆盖外部修改 | 保存前比较 mtime/内容哈希，冲突时提示选择保留版本；补 Rust 集成测试 |
| B1-5 | 未保存关闭确认 | RFC-001 §4.2 仅覆盖切换笔记 | 未实现；关闭窗口可能丢失防抖窗口内输入 | 监听 close-requested，dirty 时 flush 或弹确认 |
| B1-6 | 多标签/多窗口 | RFC-001 §7 非目标 | 未实现；单笔记会话 | 有明确用户需求后开新 RFC，评估状态模型与内存占用 |

## B2. 工程质量与安全

| ID | 项 | 证据 | 现状 | 收尾标准 |
|---|---|---|---|---|
| B2-1 | 原子写入 | `notes.rs` 计划用 `fs::write` | 写入中断可能留下截断文件 | 同目录临时文件 + rename；Windows 覆盖策略单测 |
| B2-2 | UI 组件测试 | 当前只测 core/application/adapters | React 组件靠类型检查与手工验收 | 引入 Testing Library，覆盖空态、搜索、新建、保存失败提示 |
| ~~B2-3~~ | 文档门禁进 CI | `.github/workflows/ci.yml` 的 `docs` job | **Completed 2026-10-23**：push/PR 自动执行 `node scripts/check-eng-docs.cjs` | CI 在 PR 上运行 `bun run check:docs`，失败阻断合并 |
| B2-4 | 跨平台路径测试 | 仅在 Windows 开发 | macOS/Linux 未验证 | CI 矩阵跑 `cargo test` 与 `bun run test` |

## B3. 环境与依赖

| ID | 项 | 说明 | 收尾标准 |
|---|---|---|---|
| B3-1 | WebView2 缺失引导 | 旧版 Windows 可能没有 WebView2 | 安装包检测并在首次启动给出下载引导 |
| B3-2 | 图标与品牌资源 | 暂用 Tauri 默认图标 | 替换为自有 1024×1024 源图并重新生成全平台图标 |
