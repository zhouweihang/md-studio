# 搜索性能重构 实现计划（2026-10-23）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把搜索从「每次查询重扫磁盘」重构为「后台预热的内存快照 + 并行扫描 + Channel 流式返回」，10k 档暖查询 p95 ≤20 ms，50k 档可打开可搜索，并建立分机器基线的回归门禁。

**Status:** Pending（文档已完成，代码未开始）。

**权威文档：** `docs/rfc/RFC-006-search-performance.md`、`docs/specs/2026-10-23-search-performance-design.md`、ADR-007~009。

---

## File Structure

- Add: `src-tauri/src/search.rs`（快照、LRU、匹配、预览、并行、取消）
- Add: `src-tauri/src/watcher.rs`（notify 监听、去抖、增量更新）
- Add: `src-tauri/src/bin/bench_search.rs`（release 基准入口）
- Add: `src/adapters/tauri/noteRepository.test.ts`
- Add: `scripts/gen-search-corpus.mjs`、`scripts/bench-search.mjs`
- Add: `docs/perf/baseline/dev-i9-14900hx.json`、`docs/perf/results/`（结果产物）
- Modify: `src-tauri/src/notes.rs`（搜索逻辑迁出，命令改薄封装）
- Modify: `src-tauri/src/lib.rs`（注册命令与状态）
- Modify: `src-tauri/Cargo.toml`（aho-corasick / rayon / notify）
- Modify: `src/application/ports.ts`、`src/application/workspace.ts`、`src/adapters/tauri/noteRepository.ts`
- Modify: `src/ui/App.tsx`、`src/ui/Sidebar.tsx`、`src/ui/app.css`
- Modify: `src/ui/App.test.tsx`、新增 `Sidebar` 渲染测试（如拆分组件）
- Modify: `README.md`、`docs/backlog.md`、`docs/architecture/C{1,2,3}-*.md`、`knowledge.md`、RFC-006 §6/§8

---

## Task 0: 文档落位

**Files:** RFC-006、ADR-007~009、spec、plan、backlog、C1/C2/C3、README、knowledge.md

- [x] **Step 1**: 裁决 spec Q1~Q10 并回填。
- [x] **Step 2**: 写 RFC-006（REQ-022~030）。
- [x] **Step 3**: 写 ADR-007（快照）、ADR-008（流式）、ADR-009（监听），更新 ADR 索引、ADR-004 状态、ADR-003 Amendment。
- [x] **Step 4**: 写本 plan。
- [x] **Step 5**: 更新 backlog B4-1~B4-9、C1/C2/C3、README、knowledge.md。
- [x] **Step 6**: `node scripts/check-eng-docs.cjs` PASS。

## Task 1: 基准设施（先量再改）

**Files:** `scripts/gen-search-corpus.mjs`、`scripts/bench-search.mjs`、`src-tauri/src/bin/bench_search.rs`、`docs/perf/baseline/dev-i9-14900hx.json`

- [x] **Step 1**: 语料生成器：固定种子，三档 1k/10k/50k；大小分布 70% 2–8 KB、20% 8–32 KB、10% 32–256 KB；30% 中文正文；含一个 5 MiB 边界文件。
- [x] **Step 2**: 把 `search_notes_blocking` 从 `notes.rs` 抽到 `src/search.rs`，`notes.rs` 只保留命令封装；`cargo test` 全绿（22 passed）。
- [x] **Step 3**: `bench_search` 输出 `{ coldMs, warmP50Ms, warmP95Ms, warmMinMs, warmMaxMs, hits, runs }`（`peakRssMb` 暂为 null，待 L2 增加 RSS 采集），支持 `--repeat/--warmup/--json`。
- [x] **Step 4**: 采集改造前基线（1k / 10k / 50k），写入 `docs/perf/baseline/dev-i9-14900hx.json`。
- [x] **Step 5**: `cargo run --release --bin bench_search -- --root <corpus-1k> --query needle --json` 可复现。

**验收**：基线已入库——暖 p50 1k 415.5 ms、10k 4.05 s、50k 20.28 s；冷启动 415.9 ms / 4.08 s / 20.19 s。1k 高于 spec §2.2 的 313 ms 是因为本语料 27.2 MiB（spec 语料约 0.32 MiB），属预期差异。

**备注**：为采集 50k，`MAX_NOTES` 已提前提升到 50,000（Task 7 仍负责侧栏虚拟化与 UI 测试）；语料位于 `md-studio/target/bench-corpus/`（已忽略）。

## Task 2: L0 热点优化（REQ-022）

**Files:** `src-tauri/src/search.rs`、`src-tauri/src/notes.rs`（`list_notes` 排序）

- [x] **Step 1**: 先补测试：ASCII 大小写、中文、多匹配、无匹配、片段省略号、路径边界、`list_notes` 排序。
- [x] **Step 2**: 排序 key 预计算 + `sort_by_cached_key`（`notes.rs`）。
- [x] **Step 3**: 全 ASCII needle 走 `aho-corasick` 的 `ascii_case_insensitive`；非 ASCII 保留 `to_lowercase` 语义。
- [x] **Step 4**: preview 用 `char_indices` 切片，仅命中行生成，不再 `Vec<char>` 全行拷贝。
- [x] **Step 5**: 搜索路径去掉重复 `canonicalize`，改为词法校验 + `symlink_metadata` 拒绝符号链接；`search_target_rejects_traversal_absolute_and_non_markdown` 覆盖。
- [x] **Step 6**: `cargo test --locked --all-targets`（26 passed）；`bench_search` 对比基线：1k −30.1%、10k −28.9%、50k −26.6%（暖 p50）。

**验收**：1k 暖 p50 415.5 → 290.3 ms（−30.1%），远超 ≥10% 目标；匹配语义、中文、大小写与片段用例全绿。

## Task 3: L1 并行扫描（REQ-023）

**Files:** `src-tauri/src/search.rs`

- [ ] **Step 1**: 笔记切块并行扫描，线程数 `min(available_parallelism, 8)`。
- [ ] **Step 2**: 全局命中上限用原子计数或局部收集 + 主线程截断，保证 ≤50 条。
- [ ] **Step 3**: 与串行结果做逐条比对测试（路径、行号、片段）。
- [ ] **Step 4**: 基准 1k 档 ≤120 ms。

**验收**：结果集完全一致；Windows Defender 开启状态下实测达标。

## Task 4: L2 内存快照与预热（REQ-024 / REQ-025）

**Files:** `src-tauri/src/search.rs`、`src-tauri/src/notes.rs`、`src-tauri/src/lib.rs`

- [ ] **Step 1**: `Snapshot` / `CacheEntry` / `SnapshotState` 与 LRU 单测（预算、淘汰顺序、字节记账）。
- [ ] **Step 2**: 工作区打开后 `spawn_blocking` 预热，64 篇一批，更新状态。
- [ ] **Step 3**: 查询走快照；未就绪、被淘汰、超限或非 UTF-8 回退读盘。
- [ ] **Step 4**: `snapshot_status` 命令返回 `{ state, total, cached, bytes, degradedReason }`。
- [ ] **Step 5**: 切换工作区丢弃快照；预算可由 `MDSTUDIO_SEARCH_CACHE_MB` 覆盖。
- [ ] **Step 6**: 基准 10k 档暖 p95 ≤20 ms；冷启动记录到基线。

**验收**：降级路径正确；256 MB 预算下淘汰不改变结果；状态可见。

## Task 5: 流式契约与前端迁移（REQ-026 / REQ-029）

**Files:** `src-tauri/src/search.rs`、`src-tauri/src/lib.rs`、`src/application/ports.ts`、`src/application/workspace.ts`、`src/adapters/tauri/noteRepository.ts`、`src/ui/App.tsx`、`src/ui/Sidebar.tsx`、测试文件

- [ ] **Step 1**: 定义 `SearchEvent`、`request_id`、`search_cancel`；旧一次性命令删除。
- [ ] **Step 2**: 每批 ≤16 条，`Done` 唯一；取消检查每批一次。
- [ ] **Step 3**: 前端端口与适配器迁移到 `searchNotes(root, query, onBatch)`。
- [ ] **Step 4**: App effect 管理 requestId、清空旧结果、卸载取消；自适应防抖 80/120/200 ms。
- [ ] **Step 5**: 每批对 ≤50 条稳定排序；`Done` 显示总数/耗时/来源。
- [ ] **Step 6**: 测试 mock `Channel`；`noteRepository.test.ts` 断言事件映射；App 测试覆盖流式与取消。
- [ ] **Step 7**: `bun run typecheck && bun run test`。

**验收**：旧命令无引用；取消无迟到副作用；首屏 p95 ≤100 ms（手动计时 @10k）。

## Task 6: 文件监听与失效（REQ-027）

**Files:** `src-tauri/src/watcher.rs`、`src-tauri/src/search.rs`、`src-tauri/src/lib.rs`

- [ ] **Step 1**: notify 递归监听，过滤 Markdown 与忽略目录。
- [ ] **Step 2**: 300 ms 去抖；create/modify/remove/rename 增量更新。
- [ ] **Step 3**: 失败/溢出 → `Degraded`；查询前 mtime+size 校验。
- [ ] **Step 4**: 手动刷新全量对账。
- [ ] **Step 5**: 集成测试：外部写入、删除、rename 原子保存三种路径。
- [ ] **Step 6**: `cargo test --locked --all-targets`。

**验收**：外部编辑 500 ms 内可见；监听被禁用时结果仍正确、状态为 `Degraded`。

## Task 7: 50k 上限与侧栏虚拟化（REQ-028）

**Files:** `src-tauri/src/notes.rs`、`src/ui/Sidebar.tsx`、`src/ui/app.css`、测试文件

- [ ] **Step 1**: `MAX_NOTES` 已在 Task 1 提前改为 50,000；仍需补上限测试与超限错误信息。
- [ ] **Step 2**: 侧栏固定行高窗口化（24 px、overscan 20、`ResizeObserver`、可注入 `viewportHeight`）。
- [ ] **Step 3**: 目录默认折叠；搜索过滤只渲染匹配项；保留 `tree/treeitem` 与键盘导航。
- [ ] **Step 4**: 50k 语料打开工作区计时；IPC 体积与树构建耗时记录到 plan 或 RFC。
- [ ] **Step 5**: 若打开 >2 s，引入按目录懒加载并另开 B4-7 子项。

**验收**：50k 可打开可搜；UI 不冻结；无障碍角色与键盘用例通过。

## Task 8: 全量验证与回填

- [ ] **Step 1**: `node scripts/check-eng-docs.cjs`。
- [ ] **Step 2**: `bun run typecheck`、`bun run test`、`bun run build`。
- [ ] **Step 3**: `cargo test --locked --all-targets`。
- [ ] **Step 4**: `bun run tauri build`（NSIS，记录体积）。
- [x] **Step 5**: 门禁脚本 `check-search-perf.mjs` 已实现，并用基线自检 PASS 与 FAIL 两条路径；基准仍不进 CI。
- [ ] **Step 6**: 回填 RFC-006 §6/§8、README、knowledge.md、C3、backlog 状态。

---

## Self-Review

- 数据语义：快照只服务读路径；保存仍读盘比对 revision；删除/恢复/重命名语义不变。
- 与既有机制共存：路径校验保留在 read/write/delete 边界；命中上限仍为 50 条、每篇 3 条。
- 降级：预热未完成、条目被淘汰、监听失败、网络盘四种场景都必须回退实时扫描且结果正确。
- 风险：L2 与 L1 的收益可能重叠，Task 3 的基准必须在 Task 4 之前单独记录，避免归因错误。
- 未覆盖：跨平台监听、模糊/正则搜索、持久化索引，均明确留给 ADR-010 或后续 RFC。
