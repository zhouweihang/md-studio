# Backlog 待办清单（工程级）

> Status: Active | Created: 2026-10-23 | Updated: 2026-10-25 | 用途：归档跨阶段遗留项，防止遗漏；每项含证据、现状、收尾标准
> 关联：RFC-001~006、ADR-001~009

## B0. 已闭环（P0/P1/P2）

- **2026-10-23 MVP 闭环**：REQ-001~REQ-007 全部验收；前端 21 passed、Rust 7 passed、`typecheck`/`build`/NSIS 1.50 MiB/文档门禁 PASS。
- **2026-10-23 Git/CI 接入**：初始化 `main` 仓库；`.github/workflows/ci.yml` 覆盖 web 测试/类型/构建、Windows MSVC `cargo test`、文档门禁。
- **2026-10-23 RFC-002 闭环**：删除/重命名/全文搜索通过；前端 27 passed、Rust 12 passed、typecheck/build/NSIS 1.51 MiB/docs PASS。
- **2026-10-23 RFC-003 闭环**：关闭前 flush、revision 条件写入、冲突横幅通过；前端 28 passed、Rust 15 passed、typecheck/build/NSIS/docs PASS。
- **2026-10-23 RFC-004 闭环**：系统回收站删除 + 撤销 toast 通过；前端 30 passed、Rust 21 passed、typecheck/build/NSIS 1.52 MiB/docs PASS。
- **2026-10-23 RFC-005 闭环**：搜索命中高亮 + 大工作区性能测试通过；前端 36 passed、Rust 22 passed（1,001 篇 330.97ms）、typecheck/build/NSIS 1.52 MiB/docs PASS。
- **2026-10-23 RFC-006 文档落位**：性能评估 + 7 轮访谈结论固化为 spec；RFC-006、ADR-007~009、plan、B4 区块完成；门禁 PASS。代码未开始，L0~L3 由 B4 区块统一跟踪。
- **2026-10-25 RFC-006 代码闭环**：B4 区块除 L3 评估外的八项全部完成（快照/LRU、并行、流式、监听、50k 虚拟化、基准门禁）；Rust 51 passed、前端 69 passed。
- **2026-10-25 公开仓库与三平台 CI**：`github.com/zhouweihang/md-studio` 转为 Public；Rust 测试矩阵覆盖 Windows / Ubuntu / macOS（macOS 非阻塞），连同前端测试/构建与文档门禁五个 job 全绿（`2de6ae8`）；本机 release 安装包实测可启动。

## B1. 产品能力遗留（MVP 之外）

| ID | 项 | 证据 | 现状 | 收尾标准 |
|---|---|---|---|---|
| ~~B1-1~~ | 删除/重命名笔记 | RFC-002 §4.1/§4.2；`src-tauri/src/notes.rs` | **Completed 2026-10-23**：工具栏对话框 + Rust `delete_note`/`rename_note`，路径校验与冲突拒绝有单测 | 侧栏提供删除/重命名，二次确认，Rust 侧同样做路径校验 + 单测 |
| ~~B1-2~~ | 全文搜索 | RFC-002 §4.3；ADR-004；`search_notes` | **Completed 2026-10-23**：Rust 行扫描，250ms 防抖，返回行号/片段；上限 50 条 | 输入 300ms 内返回命中片段；>5000 文件时用派生索引，需新 ADR |
| ~~B1-7~~ | 删除进系统回收站 | RFC-004 §4.1；ADR-006；`src-tauri/src/notes.rs` | **Completed 2026-10-23**：`trash` crate 移入系统回收站；删除返回文本快照，toast「撤销」写回原路径；回收站失败保持文件；Rust 6 个用例 + 前端 2 个用例 | 使用平台回收站 API + 撤销入口；单测覆盖失败路径 |
| ~~B1-3~~ | 文件系统监听 | ADR-003 Decision 2；ADR-009 | **Completed 2026-10-25**：监听服务搜索快照失效（见 B4 组的文件监听条目）与文件树；新增/删除/改名以 `workspace:changed` 增量推送，前端 `applyWorkspaceDelta` 就地 patch，正文修改不发事件（不干扰自动保存）；Rust 51 passed、前端 69 passed | 监听工作区变化并增量更新树；重命名/原子保存不丢事件 |
| ~~B1-4~~ | 并发编辑冲突检测 | RFC-003 §4.1；ADR-005；`src-tauri/src/notes.rs` | **Completed 2026-10-23**：FNV-1a revision 条件写入，冲突返回 `conflict:` 且不覆盖；UI 提供重新载入/覆盖保存；Rust 3 个用例 + 前端 3 个用例 | 保存前比较内容哈希，冲突时提示选择保留版本；补 Rust 集成测试 |
| ~~B1-5~~ | 未保存关闭确认 | RFC-003 §4.4；`src/ui/App.tsx` | **Completed 2026-10-23**：`onCloseRequested` 先 flush 再 `close()`，失败/冲突保持窗口；前端 close 用例覆盖 | 监听 close-requested，dirty 时 flush 或弹确认 |
| B1-6 | 多标签/多窗口 | RFC-001 §7 非目标 | 未实现；单笔记会话 | 有明确用户需求后开新 RFC，评估状态模型与内存占用 |

## B2. 工程质量与安全

| ID | 项 | 证据 | 现状 | 收尾标准 |
|---|---|---|---|---|
| B2-1 | 原子写入 | `notes.rs` 计划用 `fs::write` | 写入中断可能留下截断文件 | 同目录临时文件 + rename；Windows 覆盖策略单测 |
| ~~B2-2~~ | UI 组件测试 | `src/ui/App.test.tsx`、`src/ui/NoteDialog.test.tsx` | **Completed 2026-10-23**：Testing Library 共 13 个 UI 用例（App 10 + NoteDialog 3），覆盖空态、打开/编辑/预览、文件名过滤、内容命中高亮、关闭前 flush、冲突横幅/覆盖/重载、删除确认与回收站撤销、重命名、Escape，以及「新建 → 出现在树 → 打开为空编辑器」 | 引入 Testing Library，覆盖空态、搜索、新建、保存失败提示 |
| ~~B2-3~~ | 文档门禁进 CI | `.github/workflows/ci.yml` 的 `docs` job | **Completed 2026-10-23**：push/PR 自动执行 `node scripts/check-eng-docs.cjs` | CI 在 PR 上运行 `bun run check:docs`，失败阻断合并 |
| ~~B2-4~~ | 跨平台路径测试 | `.github/workflows/ci.yml` | **Completed 2026-10-25（部分）**：Rust 测试矩阵覆盖 `windows-latest` / `ubuntu-latest` / `macos-latest`，前端测试与构建在 Ubuntu；macOS 仍为 `continue-on-error`（FSEvents 去抖语义待观察），因此只有 Windows/Linux 阻断 | CI 矩阵跑 `cargo test` 与 `bun run test` |
| ~~B2-5~~ | 搜索性能基线 | RFC-005 §4.2；ADR-004 | **Completed 2026-10-23**：1,001 篇笔记搜索实测 330.97ms（debug），测试断言 <10s 并打印耗时。RFC-006 将用三档基准与分机器基线取代单一护栏（见搜索基准条目） | 1,000 篇工作区有可重复耗时断言，数量级退化会被发现 |

## B3. 环境与依赖

| ID | 项 | 说明 | 收尾标准 |
|---|---|---|---|
| B3-1 | WebView2 缺失引导 | 旧版 Windows 可能没有 WebView2 | 安装包检测并在首次启动给出下载引导 |
| B3-2 | 图标与品牌资源 | 暂用 Tauri 默认图标 | 替换为自有 1024×1024 源图并重新生成全平台图标 |

## B4. 搜索性能（RFC-006）

| ID | 项 | 证据 | 现状 | 收尾标准 |
|---|---|---|---|---|
| ~~B4-1~~ | L0 扫描热点优化 | RFC-006 §4.2；REQ-022 | **Completed 2026-10-23**：aho-corasick ASCII 免分配匹配、按需片段、排序 key 预计算、搜索路径去 canonicalize；暖 p50 1k 415.5→290.3 ms（−30.1%）、10k 4050.8→2880.8 ms（−28.9%）、50k 20279.5→14881.8 ms（−26.6%） | 1k 档 release 比基线下降 ≥10%；匹配语义与黄金结果一致 |
| ~~B4-2~~ | L1 并行扫描 | RFC-006 §4.3；REQ-023 | **Completed 2026-10-24**：`std::thread::scope` 切块扫描 + 块序拼接截断（顺序与串行一致）；`walk` 拆成串行枚举 + 并行 stat；暖 p50 1k 290.3→83.7 ms、10k 2880.8→793.8 ms、50k 14881.8→4322.1 ms；`parallel_scan_matches_serial_scan_hit_for_hit` 逐条比对，Rust 27 passed | 1k 档 ≤120 ms（本机 release）；结果集与串行逐条一致 |
| ~~B4-3~~ | L2 内存快照 + LRU | RFC-006 §4.4；ADR-007；REQ-024 | **Completed 2026-10-24，SLO 于 2026-10-25 裁决**：进程内只读快照 + LRU 预算（默认 256 MB，`MDSTUDIO_SEARCH_CACHE_MB` 可覆盖）；暖 p50 1k 83.7→4.1 ms、10k 793.8→19.8 ms、50k 4,322.1→2,509.8 ms；p95 15.3 / 28.4 / 2,657.1 ms 对修订后预算 22.7 / 41.6 ms（`20 ms + 0.10 ms/MiB`）均通过，余量约 32%。原「10k ≤20 ms」绝对阈值废止——215.6 MiB 正文要 10.8 GB/s 且零开销，实测整篇扫描 p50 已 11–12 GB/s；理由见 plan Task 4「SLO 裁决」与 RFC-006 §0/§6 | 零缓存/部分缓存/全缓存结果逐条一致；预算记账与淘汰单测 |
| ~~B4-4~~ | 后台预热与状态机 | RFC-006 §4.4；REQ-025 | **Completed 2026-10-24**：`list_notes` 后后台预热（64 篇/批，1k 225 ms、10k 2.21 s、50k 8.25 s）；`Warming/Ready/Degraded` 与 total/cached/bytes/budget 经 `snapshot_status` 暴露；未就绪/降级回退实时扫描 | 打开工作区不阻塞；三态可见；失败降级 |
| ~~B4-5~~ | 流式搜索契约 | RFC-006 §4.5；ADR-008；REQ-026 | **Completed 2026-10-25**：`search_notes(root, query, request_id, on_event)` 改为 Channel 流式（批次 ≤16、唯一 `Done`、`source` 三态）并新增 `search_cancel`；端口改为 `searchNotes(root, query, requestId, onBatch)`，App 管理 requestId、自适应防抖 80/120/200 ms、卸载/换词时取消；10k 首批 0.65 ms p50 / 0.91 ms p95；Rust 42 passed、前端 47 passed | 旧 `search_notes` 退役；逐批渲染；取消无迟到副作用 |
| ~~B4-6~~ | 文件监听 + mtime 兜底 | RFC-006 §4.6；ADR-009；REQ-027 | **Completed 2026-10-25**：`watcher.rs` 用 `notify` 8 递归监听 + 300 ms 去抖，去抖后把事件归约为磁盘现状（重读 / 删除 / 目录连带清理）；初始化失败、溢出或连续读失败置 `Degraded`，查询前 mtime+size 校验并后台重预热；手动刷新按 mtime 对账；7 条集成测试覆盖外部写入、删除、目录删除、原子保存、忽略目录、降级与换工作区；Rust 49 passed | 外部编辑 500 ms 内可见；监听失败自动降级 |
| ~~B4-7~~ | 50k 上限 + 侧栏虚拟化 | RFC-006 §4.7；REQ-028 | **Completed 2026-10-25**：`MAX_NOTES` 50,000 与限流单测；侧栏固定 24 px 行高窗口化（overscan 20、`ResizeObserver`、`viewportHeight` 可注入），目录默认折叠，搜索只渲染匹配；50k 枚举 1,029 ms、IPC 5.10 MB、树构建 149 ms，DOM 恒为 ~50 行（未触发 2 s 懒加载）；`Sidebar` 6 条用例覆盖窗口、滚动、折叠、搜索与键盘 | 50k 工作区可打开、可搜索；树虚拟化；键盘与无障碍不回归 |
| ~~B4-8~~ | 基准脚本 + 基线门禁 | RFC-006 §10；REQ-030 | **Completed 2026-10-23，基线于 2026-10-24 随 L2 重采**：语料生成器、`bench_search`（`--no-snapshot` 可复现磁盘路径）、编排脚本与基线入库（暖 p50 1k 4.1 ms、10k 19.8 ms、50k 2.51 s）；`check-search-perf.mjs` 按中位数 ≤1.2×、p95 ≤2× 判定，PASS/FAIL 两条路径已自检 | 1k/10k/50k 可复现；基线 JSON 入库；1.2× 中位数 / 2× p95 可判定 |
| B4-9 | L3 触发评估（ADR-010） | spec §5 L3 | 未开始 | 触发条件成文，附候选对比维度；不实现索引 |

## B5. 发布工程

| ID | 项 | 说明 | 收尾标准 |
|---|---|---|---|
| B5-1 | 版本号与发布节奏 | **策略已裁决 2026-10-25（方案 ②）**：应用版本走产品语义化 `0.x`，`0.1.0` 即首个公开版本，不随 RFC 阶段号跳动；RFC-001~006 的 `Target`（`v0.1.0`~`v0.6.0`）只是内部开发台阶，不改写 `package.json` / `tauri.conf.json`。剩余：仓库仍无 tag 与 Release | 打 `v0.1.0` tag 并发布 Release，挂上 NSIS 安装包与变更说明 |
| B5-2 | 代码签名 | NSIS 安装包未签名，SmartScreen 报「未知发布者」 | 取得证书并在 `tauri.conf.json` 配置签名，或明确放弃并在 README 写明 |
| B5-3 | macOS / Linux 打包 | `bundle.targets` 只有 `nsis` | 产出 `.dmg` 与 `.AppImage`/`.deb`，或明确只支持 Windows |
