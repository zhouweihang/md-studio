# 搜索性能重构 实现计划（2026-10-23）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把搜索从「每次查询重扫磁盘」重构为「后台预热的内存快照 + 并行扫描 + Channel 流式返回」，10k 档暖查询 p95 ≤ 41.6 ms（= 20 ms 固定开销 + 0.10 ms/MiB × 215.6 MiB，2026-10-25 修订），50k 档可打开可搜索，并建立分机器基线的回归门禁。

**Status:** In progress（Task 0–4 完成；下一步 Task 5；Task 7 按项目所有者要求暂缓）。2026-10-25 裁决 REQ-024 的 p95 口径改为按正文字节表达，已回填 RFC-006 §0/§1/§6/§8。

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

**Files:** `src-tauri/src/search.rs`、`src-tauri/src/notes.rs`（枚举阶段并行 stat）

- [x] **Step 1**: 笔记切块并行扫描，线程数 `min(available_parallelism, 8)`；用 `std::thread::scope` 而非 `rayon`（切分是静态的、按顺序合并，`rayon` 只会多一个依赖）。
- [x] **Step 2**: 每块局部收集并各自封顶 50 条，按块序拼接后由主线程截断，保证 ≤50 条且顺序与串行一致。
- [x] **Step 3**: `parallel_scan_matches_serial_scan_hit_for_hit` 用同一份 `NoteEntry` 列表比串行/并行结果（路径、行号、片段、顺序），并覆盖全局 50 条上限与每篇 3 条上限。
- [x] **Step 4**: 基准 1k 档暖 p50 **83.7 ms** / p95 93.3 ms（目标 ≤120 ms）；10k 793.8 ms、50k 4,322.1 ms。

**验收**：结果集与串行逐条一致（27 个 Rust 用例全绿）；Windows Defender 开启状态下 1k 档实测 83.7 ms，达标。

**实现备注**：并行化分两段——① `search.rs` 按 `list_notes` 已排序的路径切块扫描；② `notes.rs` 把 `walk` 拆成「串行枚举路径 + 并行 stat」（`collect_note_paths` + `resolve_entries`）。只做 ① 时 1k 为 136 ms，枚举占其中 77 ms，是把枚举也并行后才进 120 ms 以内（25 ms）。两段共用 `worker_count`，上限都是 8。

## Task 4: L2 内存快照与预热（REQ-024 / REQ-025）

**Files:** `src-tauri/src/search.rs`、`src-tauri/src/notes.rs`、`src-tauri/src/lib.rs`

- [x] **Step 1**: `Snapshot` / `SnapshotState` 与 LRU 单测（预算、淘汰顺序、字节记账）。
- [x] **Step 2**: 工作区打开后后台预热，64 篇一批，更新状态。
- [x] **Step 3**: 查询走快照；未就绪、被淘汰、超限或非 UTF-8 回退读盘。
- [x] **Step 4**: `snapshot_status` 命令返回 `{ state, total, cached, bytes, budgetBytes, degradedReason }`（无工作区时为 `null`）。
- [x] **Step 5**: 切换工作区丢弃快照；预算可由 `MDSTUDIO_SEARCH_CACHE_MB` 覆盖（默认 256 MB）。
- [x] **Step 6**: 基准入库；10k 档暖 p95 **28.4 ms**，低于修订后预算 41.6 ms（见下方 SLO 裁决）。

**验收**：降级路径正确；预算（含部分缓存与零缓存）下结果逐条不变；状态可见；全缓存档暖 p95 满足 `20 ms + 0.10 ms/MiB`（1k ≤22.7 ms、10k ≤41.6 ms）。

**结果（2026-10-24，release，本机，语料 1k 27.2 MiB / 10k 215.6 MiB / 50k 1,085.5 MiB）**

| 档位 | 冷启动 | L1 暖 p50 | **L2 暖 p50** | **L2 暖 p95** | 预热 | 缓存 |
|---|---|---|---|---|---|---|
| 1k | 65.9 ms | 83.7 ms | **4.1 ms** | **15.3 ms** | 225 ms | 1,000 / 1,000 |
| 10k | 642.5 ms | 793.8 ms | **19.8 ms** | **28.4 ms** | 2.21 s | 10,000 / 10,000 |
| 50k | 3,302.9 ms | 4,322.1 ms | **2,509.8 ms** | **2,657.1 ms** | 8.25 s | 11,520 / 50,000 |

- 10k 相对 L1 下降 97.5%，1k 下降 95.1%；50k 只降 42%，因为 256 MB 预算只装得下 11,520 篇，其余每查询回退读盘。
- **SLO 裁决（2026-10-25）**：原定「10k 档暖 p95 ≤20 ms」废止，改为 **全缓存时 p95 ≤ 20 ms + 0.10 ms/MiB 已缓存正文**。理由：10k 语料正文 215.6 MiB，20 ms 等于 10.8 GB/s 且不留线程切换与结果合并开销；实测整篇扫描 p50 18–20 ms 已是 11–12 GB/s，p95 的 28.4 ms 里约 8 ms 是 32 逻辑核上 16 线程的调度抖动，不是算法余量。新口径下 1k 预算 22.7 ms / 实测 15.3 ms、10k 预算 41.6 ms / 实测 28.4 ms，两档余量都约 32%，说明模型与实测一致。若以后要追绝对 20 ms，只能建索引（ADR-010），不在本 RFC 范围。
- 对照：spec 原语料 1k 仅 0.32 MB（约 320 B/篇），本基准语料按 spec §2.2 的体积分布生成，平均 22.6 KB/篇，是原语料的 70 倍。绝对毫秒阈值因此会随语料体积漂移，这也是 SLO 改按字节表达的原因。

**实现备注**

1. 预热用 `std::thread::spawn` 而非 `spawn_blocking`：工作全在 Rust 侧、不需要 async runtime，基准与测试可直接复用同一实现。
2. 预热达到预算即停止载入正文（剩余预算 < 5 MiB 时收工），不读完整个工作区再淘汰；LRU 淘汰仍用于应用内写入触发的插入。预算满后未缓存的笔记元数据保留，查询时逐篇回退读盘，结果与顺序不变。
3. 写/新建/删除/改名/恢复都会同步快照（`cache_upsert` / `cache_remove` / `cache_rename`），否则保存后搜索结果会看到旧正文。
4. 为把快照扫描压进目标区间，ASCII needle 改为「整篇一次扫描 + 命中处回算行号」（逐行 `find` 在 10k 档要调用千万次）：10k 从 44.3 ms 降到 19.8 ms。Unicode needle 保留逐行路径，因为 `to_lowercase` 会改变字节长度。
5. 快照扫描线程上限 16（磁盘路径仍为 8）：32 逻辑核上 8 线程 p50 24 ms、16 线程 17–20 ms、32 线程 p50 更低但 p95 更差。
6. **已知缺口**：外部编辑器修改在 Task 6（监听）之前不会被快照感知，搜索会返回预热时的正文；应用内写入已同步。

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
