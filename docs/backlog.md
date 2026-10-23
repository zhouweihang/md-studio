# Backlog 待办清单（工程级）

> Status: Active | Created: 2026-10-23 | Updated: 2026-10-23 | 用途：归档跨阶段遗留项，防止遗漏；每项含证据、现状、收尾标准
> 关联：RFC-001~005、ADR-001~006

## B0. 已闭环（P0/P1/P2）

- **2026-10-23 MVP 闭环**：REQ-001~REQ-007 全部验收；前端 21 passed、Rust 7 passed、`typecheck`/`build`/NSIS 1.50 MiB/文档门禁 PASS。
- **2026-10-23 Git/CI 接入**：初始化 `main` 仓库；`.github/workflows/ci.yml` 覆盖 web 测试/类型/构建、Windows MSVC `cargo test`、文档门禁。
- **2026-10-23 RFC-002 闭环**：删除/重命名/全文搜索通过；前端 27 passed、Rust 12 passed、typecheck/build/NSIS 1.51 MiB/docs PASS。
- **2026-10-23 RFC-003 闭环**：关闭前 flush、revision 条件写入、冲突横幅通过；前端 28 passed、Rust 15 passed、typecheck/build/NSIS/docs PASS。
- **2026-10-23 RFC-004 闭环**：系统回收站删除 + 撤销 toast 通过；前端 30 passed、Rust 21 passed、typecheck/build/NSIS 1.52 MiB/docs PASS。
- **2026-10-23 RFC-005 闭环**：搜索命中高亮 + 大工作区性能测试通过；前端 36 passed、Rust 22 passed（1,001 篇 330.97ms）、typecheck/build/NSIS 1.52 MiB/docs PASS。

## B1. 产品能力遗留（MVP 之外）

| ID | 项 | 证据 | 现状 | 收尾标准 |
|---|---|---|---|---|
| ~~B1-1~~ | 删除/重命名笔记 | RFC-002 §4.1/§4.2；`src-tauri/src/notes.rs` | **Completed 2026-10-23**：工具栏对话框 + Rust `delete_note`/`rename_note`，路径校验与冲突拒绝有单测 | 侧栏提供删除/重命名，二次确认，Rust 侧同样做路径校验 + 单测 |
| ~~B1-2~~ | 全文搜索 | RFC-002 §4.3；ADR-004；`search_notes` | **Completed 2026-10-23**：Rust 行扫描，250ms 防抖，返回行号/片段；上限 50 条 | 输入 300ms 内返回命中片段；>5000 文件时用派生索引，需新 ADR |
| ~~B1-7~~ | 删除进系统回收站 | RFC-004 §4.1；ADR-006；`src-tauri/src/notes.rs` | **Completed 2026-10-23**：`trash` crate 移入系统回收站；删除返回文本快照，toast「撤销」写回原路径；回收站失败保持文件；Rust 6 个用例 + 前端 2 个用例 | 使用平台回收站 API + 撤销入口；单测覆盖失败路径 |
| B1-3 | 文件系统监听 | ADR-003 Decision 2 | 未实现；外部新增文件需手动刷新 | 监听工作区变化并增量更新树；重命名/原子保存不丢事件 |
| ~~B1-4~~ | 并发编辑冲突检测 | RFC-003 §4.1；ADR-005；`src-tauri/src/notes.rs` | **Completed 2026-10-23**：FNV-1a revision 条件写入，冲突返回 `conflict:` 且不覆盖；UI 提供重新载入/覆盖保存；Rust 3 个用例 + 前端 3 个用例 | 保存前比较内容哈希，冲突时提示选择保留版本；补 Rust 集成测试 |
| ~~B1-5~~ | 未保存关闭确认 | RFC-003 §4.4；`src/ui/App.tsx` | **Completed 2026-10-23**：`onCloseRequested` 先 flush 再 `close()`，失败/冲突保持窗口；前端 close 用例覆盖 | 监听 close-requested，dirty 时 flush 或弹确认 |
| B1-6 | 多标签/多窗口 | RFC-001 §7 非目标 | 未实现；单笔记会话 | 有明确用户需求后开新 RFC，评估状态模型与内存占用 |

## B2. 工程质量与安全

| ID | 项 | 证据 | 现状 | 收尾标准 |
|---|---|---|---|---|
| B2-1 | 原子写入 | `notes.rs` 计划用 `fs::write` | 写入中断可能留下截断文件 | 同目录临时文件 + rename；Windows 覆盖策略单测 |
| B2-2 | UI 组件测试 | 当前只测 core/application/adapters | React 组件靠类型检查与手工验收 | 引入 Testing Library，覆盖空态、搜索、新建、保存失败提示 |
| ~~B2-3~~ | 文档门禁进 CI | `.github/workflows/ci.yml` 的 `docs` job | **Completed 2026-10-23**：push/PR 自动执行 `node scripts/check-eng-docs.cjs` | CI 在 PR 上运行 `bun run check:docs`，失败阻断合并 |
| B2-4 | 跨平台路径测试 | 仅在 Windows 开发 | macOS/Linux 未验证 | CI 矩阵跑 `cargo test` 与 `bun run test` |
| ~~B2-5~~ | 搜索性能基线 | RFC-005 §4.2；ADR-004 | **Completed 2026-10-23**：1,001 篇笔记搜索实测 330.97ms（debug），测试断言 <10s 并打印耗时 | 1,000 篇工作区有可重复耗时断言，数量级退化会被发现 |

## B3. 环境与依赖

| ID | 项 | 说明 | 收尾标准 |
|---|---|---|---|
| B3-1 | WebView2 缺失引导 | 旧版 Windows 可能没有 WebView2 | 安装包检测并在首次启动给出下载引导 |
| B3-2 | 图标与品牌资源 | 暂用 Tauri 默认图标 | 替换为自有 1024×1024 源图并重新生成全平台图标 |
