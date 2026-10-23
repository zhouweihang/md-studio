# 搜索性能优化 需求与决策快照（RFC-006 输入）

> Status: Draft（2026-10-23 访谈完成，待评审）｜索引：`RFC-006`（待建）｜关联 REQ：REQ-022~REQ-030（预留）
> 收益：把 10k/50k 工作区的搜索从「每次查询重扫磁盘」降到毫秒级，并建立可验收的性能门禁
> 位置：本文档为需求与决策快照；权威方案落地后将写入 `docs/rfc/RFC-006-search-performance.md`，决策记录拆为 ADR-007~ADR-009
> 一致性声明：与 ADR-003（文件系统唯一事实源）、ADR-005（revision 乐观并发）、ADR-006（回收站撤销）兼容；与 ADR-004（实时扫描、不建索引）冲突的部分由 ADR-007 明确取代，取代关系见 §6
> 变更范围：本轮只新增本文件，不改代码、不改其他文档

## 0. 这份文档是什么

2026-10-23 对「搜索性能还能不能更快、能否做到行业最好」做了一次评估，并进行了 7 轮访谈确认取舍。本文档把评估证据、承诺指标、分层方案、接口变更、文档产出与验收方式固化成可评审的快照，作为后续 RFC-006、ADR-007~009、实现计划与 backlog 的唯一输入。

本文档**不是**最终 RFC，也不代表方案已批准；它记录「已经确定什么」以及每项裁决的理由。少数实现细节留待 RFC-006 评审收敛，但不再有未决的方向性问题。

## 1. 访谈结论一览

| # | 议题 | 结论 |
|---|---|---|
| 1 | 文档组合 | 全套 + backlog 拆分：RFC-006 + ADR-007~009 + spec + plan + backlog 条目 |
| 2 | 覆盖层级 | L0 热点优化、L1 并行、L2 内存快照、L3 索引评估，四层全覆盖 |
| 3 | 目标规模 | 1k / 10k / 50k 三档 |
| 4 | 文档定位 | 承诺性 SLO + 验收，不是纯备忘 |
| 5 | ADR-004 处理 | 新 ADR 取代 ADR-004（Superseded），重新定义「冷路径扫描 + 暖路径快照」 |
| 6 | 缓存范围 | 预算内全量缓存正文；超预算按 LRU 淘汰，淘汰项回退读盘 |
| 7 | 缓存上限 | 默认 256 MB，可配置；淘汰只影响性能，不影响正确性 |
| 8 | 失效策略 | 文件监听（notify）+ 手动刷新；监听不可用时查询前 mtime 校验兜底 |
| 9 | 结果契约 | 流式增量返回，不再是一次性 `SearchHit[]` |
| 10 | 基准方式 | `cargo test` 小规模护栏 + 独立脚本跑 10k/50k 大基准 |
| 11 | 回归阈值 | 中位数 ≤ 1.2× 基线，p95 ≤ 2× 基线 |
| 12 | 防抖 | 自适应：80 / 120 / 200 ms，按快照状态与规模切换 |
| 13 | MAX_NOTES | 从 10,000 提高到 50,000 |
| 14 | 依赖策略 | 不限，按效果选；RFC 阶段给出选型对比 |
| 15 | ADR 拆分 | 3 份：缓存策略、流式契约、文件监听 |
| 16 | spec 位置 | 按项目门禁：`docs/specs/YYYY-MM-DD-*-design.md`（放弃原提议的 `*-spec.md` 命名） |
| 17 | 预热时机 | 打开工作区即后台预热，不阻塞 UI；预热完成前查询走实时扫描 |
| 18 | 命令兼容 | 新流式命令取代旧的 `search_notes`，前端与测试全部迁移 |
| 19 | 绝对目标 | 10k 档暖查询 p95 ≤ 20 ms |
| 20 | 侧栏渲染 | 需要虚拟化，否则 50k 节点全量 DOM 会卡 |
| 21 | L3 定位 | 直接列为未来 ADR（ADR-010 预留），本次不选型 |

## 2. 现状与实测证据

### 2.1 测量环境

| 项 | 值 |
|---|---|
| CPU | Intel Core i9-14900HX（24 核 / 32 线程） |
| 内存 | 47.7 GB |
| 磁盘 | Fanxiang S790MAX 1 TB SSD |
| OS | Windows build 26300 |
| rustc / cargo | 1.96.0 |
| Node / bun | 24.22.1 / 1.3.14 |

### 2.2 实测数字

命令：

```bash
cd md-studio/src-tauri
cargo test --release --locked search_notes_handles_a_large_workspace_within_budget -- --nocapture
```

结果：

| 构建 | 语料 | 耗时 |
|---|---|---|
| release | 1,001 篇，约 0.32 MB 正文 | **313.01 ms** |
| debug | 同一语料 | 330.97 ms |

debug 只比 release 慢约 6%。Rust 字符串处理与哈希在 debug 下通常慢 5–30 倍，这说明**计算部分占比不到 10%，90% 以上是文件系统调用与磁盘**。等价吞吐约 1 MB/s，比 SSD 顺序读低两个数量级。

### 2.3 每次查询的调用链

`search_notes_blocking`（`src-tauri/src/notes.rs:500`）每次查询对每篇笔记执行：

| 步骤 | 位置 | 系统调用 |
|---|---|---|
| 递归遍历目录 | `walk`（`notes.rs:201`） | `read_dir` + `file_type` |
| 读取修改时间 | `entry_for`（`notes.rs:190`） | `metadata` |
| 路径规范化与越界校验 | `resolve_existing`（`notes.rs:119`） | `canonicalize` + `is_file`（再 stat） |
| 读取全文 | `search_notes_blocking` | `read_to_string` |
| 逐行大小写折叠 | `line.to_lowercase().contains(...)` | 每行一次堆分配 |
| 生成片段 | `build_preview`（`notes.rs:545`） | 命中行再做 `Vec<char>` 全行拷贝 |

1,001 篇 ≈ 4,000+ 次文件系统调用，**每次按键都要重来一遍**。前端还有 250 ms 防抖（`src/ui/App.tsx:291`），所以感知延迟下限 = 250 ms + 搜索耗时。

### 2.4 现有约束

| 常量 | 值 | 位置 |
|---|---|---|
| `MAX_FILE_BYTES` | 5 MiB | `notes.rs:7` |
| `MAX_NOTES` | 10,000 | `notes.rs:8` |
| `MAX_DEPTH` | 12 | `notes.rs:9` |
| `MAX_SEARCH_HITS` | 50 | `notes.rs:11` |
| `MAX_HITS_PER_NOTE` | 3 | `notes.rs:12` |
| `PREVIEW_CHARS` | 90 | `notes.rs:13` |

### 2.5 结论

瓶颈不在算法，而在「每次查询重新摸盘」的架构。只优化算法，天花板是 10–20%；要进入第一梯队，必须做暖路径缓存 + 并行 + 流式。

## 3. 目标与 SLO

### 3.1 规模档位

| 档位 | 用途 | 语料假设 |
|---|---|---|
| 1k | 回归护栏、日常开发机 | 约 4 KB/篇，约 4 MB 正文 |
| 10k | 设计上限（当前 `MAX_NOTES`） | 约 4 KB/篇，约 40 MB 正文 |
| 50k | 提升后的新上限 | 约 4 KB/篇，约 200 MB 正文 |

### 3.2 SLO 表（承诺值）

| 指标 | 1k | 10k | 50k |
|---|---|---|---|
| 冷启动预热（后台，不阻塞 UI） | ≤ 0.3 s | ≤ 3 s | ≤ 10 s |
| 暖查询 p95（快照命中） | ≤ 5 ms | ≤ 20 ms | ≤ 50 ms |
| 首屏结果 p95（含防抖） | ≤ 100 ms | ≤ 100 ms | ≤ 150 ms |
| 快照内存占用 | ≤ 256 MB | ≤ 256 MB | ≤ 256 MB（可配置） |
| 降级路径（快照未就绪/被淘汰） | 不得比当前基线更慢 | 同左 | 同左 |

说明：

- 首屏结果 = 自适应防抖 + 第一批命中渲染；10k 档按 80 ms 防抖 + ≤20 ms 扫描设计。
- 50k 档的 256 MB 预算只能容纳约 200 MB 正文 + 索引元数据，边界紧张，LRU 淘汰是常态而非异常。
- 50k 档的 p95 50 ms 以「真实分布语料」（中位 4 KB）为前提；若实际命中率跌破 80%，改按「不慢于实时扫描」验收，并触发 §5 L3 的 ADR-010 评估，不再承诺 50 ms（§14 Q2）。
- 这些是**承诺值**，首轮实测后若某档确实不可达，必须在 RFC-006 评审时显式下调并记录理由，不能静默放宽。

### 3.3 回归门禁

| 指标 | 阈值 | 判定方式 |
|---|---|---|
| 暖查询中位数 | ≤ 1.2 × 基线 | 同一机器档位、同一语料、release 构建 |
| 暖查询 p95 | ≤ 2 × 基线 | 同上 |
| 冷启动预热 | 报告值（≤1.5× 为目标，单样本波动大，不作阻断） | 同上 |
| 正确性 | 命中路径/行号/片段完全一致 | 与固定语料的黄金结果比对 |

基线按机器档位分别入库（见 §10.3），跨档位不比较。

### 3.4 测量口径

- 性能数字一律以 **release** 为准；debug 只作开发参考，文档中必须标注构建类型。
- 冷启动与暖查询分开报告；暖查询在预热完成、目标笔记全部命中缓存后测量。
- 每个指标至少 30 次采样，取最近秩（nearest-rank）分位。
- 记录 CPU、内存、磁盘、OS build、Defender 实时保护状态、rustc 版本与 git commit。

## 4. 范围

### 4.1 范围内

1. L0：扫描链路热点优化（排序 key、ASCII 快路径、片段生成、去掉搜索路径上的重复 canonicalize）。
2. L1：按笔记并行的扫描。
3. L2：打开工作区后台预热的内存快照、256 MB 可配置 LRU、mtime/size 失效、文件监听、查询取消。
4. 流式搜索契约（Tauri Channel）与前端渐进渲染。
5. `MAX_NOTES` 提升到 50,000，以及随之需要的侧栏虚拟化。
6. 自适应防抖 80/120/200 ms 与首屏结果指标。
7. 基准脚本、分机器档位的基线文件、回归门禁。
8. 文档产出：RFC-006、ADR-007~009、本 spec、plan、backlog、C1/C2/C3、README、knowledge.md。

### 4.2 范围外

- 正则、模糊匹配、相关性排序（留给 ADR-010 评估）。
- 持久化倒排索引、SQLite FTS、跨会话磁盘缓存。
- 编辑器内富文本高亮（textarea 限制，另立 RFC）。
- 多标签、多窗口、协作/同步。
- 跨平台 CI 矩阵（Windows 之外的验证另开条目）。
- 遥测与性能埋点上报；本地基准是唯一观测面。

## 5. 分层方案

### L0 热点优化（不改架构）

| 改动 | 说明 |
|---|---|
| 排序 key 预计算 | `list_notes` 的 `sort_by` 里每比较一次分配两次小写字符串；改为预计算 `(lower_path, path)` 后 `sort_by_cached_key` |
| ASCII 快路径 | `needle.is_ascii()` 时用 `aho-corasick` 的 `ascii_case_insensitive` 或 `memchr::memmem`，避免对每一行做 `to_lowercase()` 分配 |
| Unicode 回退 | 非 ASCII needle 保留现有 `to_lowercase` 语义，保证中文与大小写折叠行为不变 |
| 片段按需生成 | 只对命中行生成 preview；用 `char_indices` 做字符边界切片，不再 `Vec<char>` 全行拷贝 |
| 去掉搜索路径上的 canonicalize | `walk` 已跳过符号链接且产出的相对路径只含 `Normal` 组件；搜索直接 `root.join(relative)`，canonicalize 保留在 read/write/delete 边界 |
| 计数器与早停 | 命中计数用 `usize` 局部变量，达到上限立即返回 |

预期：计算部分 2–4×，整体墙钟 10–20%（I/O 主导，见 §2.2）。

### L1 并行扫描

- 按笔记切块，用 `rayon` 或 `std::thread::scope` 并行；线程数 `min(available_parallelism, 8)`。
- 命中上限是全局的：用原子计数器或「每线程局部收集 + 主线程截断」。
- **顺序不再保证**：批次按完成顺序发出，`Done` 时由前端做一次稳定排序；这是流式契约的显式取舍。
- Windows Defender 会压低收益，验收时必须在该机器上实测，不用理论核数推断。

预期：1k 档从 313 ms 降到 60–120 ms 量级（估算，需实测）。

### L2 内存快照（拐点）

数据结构（草案）：

```rust
struct Snapshot {
    root: PathBuf,
    state: SnapshotState,      // Warming | Ready | Degraded
    notes: HashMap<PathBuf, CacheEntry>,
    bytes: usize,              // 正文总字节数，用于预算
    generation: u64,
}

struct CacheEntry {
    title: String,
    mtime_ms: u64,
    size: u64,
    text: Arc<str>,            // 命中扫描时零拷贝借用
    last_used: u64,            // LRU 计数
}
```

行为：

1. **预热**：`openWorkspace` 成功后立刻 `spawn_blocking` 遍历并读入正文，按 64 篇一批更新状态；UI 通过 `snapshot_status` 轮询（250 ms）显示「预热中 n/N」。
2. **查询**：快照 `Ready` 时在内存里并行扫描，借用 `Arc<str>`，不读盘；未就绪或条目被淘汰时回退 `read_to_string`，结果仍然正确。
3. **预算**：默认 256 MB，可由设置项/环境变量覆盖；插入新条目导致超预算时按 `last_used` 淘汰，直到回到水位以下。淘汰只影响性能，不影响正确性。
4. **取消**：Rust 保存 `AtomicU64` 最新请求号；每次查询携带 `request_id`，工作线程每处理一批检查一次，过期立即退出并释放批次。
5. **状态**：`Ready` / `Warming` / `Degraded`（监听失效或连续回退读盘）三态，UI 明示当前搜索走的是快照还是实时扫描。

内存边界：10k × 4 KB ≈ 40 MB；50k × 4 KB ≈ 200 MB，仍在 256 MB 内；若单篇 5 MiB 的极端语料占比高，LRU 会频繁淘汰，此时按 §3.2 的降级指标验收。

### L3 索引（本次只留触发条件）

只有在满足以下任一条件时才启动 ADR-010：

- 50k 档在 L2 完成后仍达不到 §3.2 的暖查询或冷启动目标。
- 产品需要正则、模糊匹配或相关性排序。
- 快照命中率长期低于 80%（工作区远大于内存预算）。

候选与评估维度（不在本次选型）：`tantivy`（完整倒排、内存/磁盘索引）、`ignore` + `grep-searcher`（ripgrep 内核、SIMD、并行遍历）、自建 trigram/FST。评估维度：延迟、内存、依赖体积、外部修改可见性、模糊/正则能力、实现与维护成本。

## 6. 决策记录（ADR 映射）

| ADR | 决策 | 与现有文档的关系 |
|---|---|---|
| **ADR-007** | 暖路径用内存快照缓存正文，冷路径保留实时扫描；预算内全量、超预算 LRU，默认 256 MB 可配置，打开工作区即后台预热 | **Supersedes ADR-004**；ADR-003 文末追加「进程内只读缓存不是持久化正文库」的 Amended 注记 |
| **ADR-008** | 搜索改为 Channel 流式契约，一次性 `search_notes` 退役；命中批次先到先显，`Done` 时稳定排序 | 取代 RFC-002/005 中的搜索接口描述 |
| **ADR-009** | 文件监听（notify）作为快照失效主路径，监听不可用时查询前 mtime+size 校验，手动刷新做全量对账 | 与 backlog B1-3（文件系统监听）合并 |
| **ADR-010**（预留） | 倒排索引/模糊搜索的引入条件与选型 | 仅在 §5 L3 触发时创建 |

`docs/adr/README.md` 需要把 ADR-004 状态改为 `Superseded by ADR-007`，并登记 ADR-007~009。

## 7. 接口与数据模型变更

### 7.1 Rust 命令

```rust
#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SearchEvent {
    Batch { hits: Vec<SearchHit> },
    Done {
        total: u32,
        truncated: bool,
        took_ms: u32,
        source: SearchSource, // Snapshot | Disk
    },
}

#[tauri::command]
pub async fn search_notes(
    root: String,
    query: String,
    request_id: u64,
    on_event: tauri::ipc::Channel<SearchEvent>,
) -> AppResult<()>;

#[tauri::command]
pub fn search_cancel(request_id: u64);

#[tauri::command]
pub fn snapshot_status(root: String) -> AppResult<SnapshotStatus>;
```

- `search_cancel` 在清空查询或切换工作区时调用，避免旧任务继续占用线程。
- 取消只保证「尽快停止」，不保证一个批次都不再发出；前端用 `request_id` 丢弃迟到批次。
- `SearchHit`、`SearchSource`、`SnapshotStatus` 的字段在 RFC-006 中定稿；`SearchHit` 结构保持 `{ path, title, line, preview }` 不变，避免前端高亮逻辑返工。

### 7.2 前端端口

```ts
export interface SearchSession {
  cancel(): void
}

export interface SearchSummary {
  total: number
  truncated: boolean
  tookMs: number
  source: 'snapshot' | 'disk'
}

searchNotes(
  root: string,
  query: string,
  onBatch: (hits: SearchHit[]) => void,
): Promise<SearchSummary>
```

- `NoteRepository` 与 `WorkspaceService` 同步改签名；`App` 的 effect 负责 `requestId` 递增、清空旧结果、卸载时取消。
- 只有 `src/adapters/tauri/noteRepository.ts` 知道 `new Channel<SearchEvent>()` 与命令名，UI 不直接 import Tauri。
- 测试策略（§14 Q3）：`@tauri-apps/api/core` 的 mock 同时提供可控的 `Channel` 假实现（记录 `onmessage`，测试里手动推送 `{ kind: 'batch' | 'done' }`）；`App.test.tsx` 用它驱动流式渲染，`noteRepository.test.ts` 单独断言 Channel 实例被传入 `invoke` 且事件映射正确。不新增第二套适配器接口。

## 8. 失效与一致性

| 事件 | 快照处理 | 可见性 |
|---|---|---|
| 应用内保存成功 | 直接替换对应条目的正文、mtime、size | 立即 |
| 外部编辑器修改 | notify 事件 → 重新读该文件 → 替换条目；无法读取则移除 | 事件延迟 + 200~500 ms 去抖 |
| 外部新建文件 | notify 事件 → 加入快照（此前搜索看不到，与现状一致） | 事件延迟 |
| 外部删除 | notify 事件 → 移除条目 | 事件延迟 |
| 应用内删除 | 删除成功后移除条目 | 立即 |
| 应用内重命名 | 移除旧键、插入新键 | 立即 |
| 原子保存（临时文件 + rename） | 监听 rename/create 事件，按路径重新读取 | 事件延迟 |
| 监听溢出/网络盘/权限失败 | 状态置 `Degraded`；每次查询前对该笔记 mtime+size 校验，不一致则重读 | 查询时 |
| 手动刷新 | 清空并按工作区重建快照（全量对账） | 立即 |

不变量：

1. 快照只服务**读路径**；`write_note` 仍然读盘比对 revision，冲突语义不变（ADR-005）。
2. 快照中的正文永远不是事实源；任何不一致以磁盘为准。
3. 监听失败不会导致错误结果，只会退化为较慢的实时扫描。
4. 不引入持久化索引与跨会话缓存。

## 9. UI/UX 变更

### 9.1 自适应防抖

| 条件 | 防抖 |
|---|---|
| 快照 `Ready` 且笔记 ≤ 2,000 | 80 ms |
| 快照 `Ready` 且笔记 > 2,000 | 120 ms |
| 快照 `Warming` 或 `Degraded` | 200 ms |

切换工作区、清空查询立即取消进行中的请求。

### 9.2 流式渲染

- 批次到达即追加到命中列表，搜索状态显示「搜索中… 已找到 N 条」。
- 每批到达后对当前结果（上限 50 条）按路径做一次稳定排序再渲染：数据量极小，排序成本可忽略，同时避免「先到先显」造成的跳动（§14 Q6）。
- `Done` 到达后显示总数、耗时与来源（快照/实时扫描），不再重排。
- 结果上限仍由后端控制；`truncated` 为真时保留现有「最多 50 条」提示（上限值在 RFC-006 中重新确认）。

### 9.3 目录树虚拟化

- 50k 节点全量 DOM 不可接受，需要虚拟列表/窗口化渲染。
- 选型（§14 Q5）：自建固定行高窗口化（行高 24 px、overscan 20 行、`ResizeObserver` 更新视口），不引入新依赖；`viewportHeight` 可注入，便于 jsdom 测试。不采用 `@tanstack/react-virtual`。
- 目录默认折叠，只渲染展开节点的子项；搜索过滤时只渲染匹配项。
- 现有 `Sidebar` 的无障碍角色（`tree`/`treeitem`）与键盘导航必须在虚拟化后保持。

### 9.4 状态可见性

- 状态栏或侧栏显示「预热中 n/N」「已缓存 X MB」「已降级：实时扫描」。
- 降级不弹错误条，只做低干扰提示，避免大工作区首次打开就报错。

## 10. 测试、基准与验收

### 10.1 测试分层

| 层 | 内容 | 运行位置 |
|---|---|---|
| 单元 | L0 匹配语义（ASCII/Unicode/大小写/中文）、LRU 淘汰、失效状态机、取消逻辑 | `cargo test` |
| 集成 | 1,001 篇语料的正确性 + 宽松耗时断言（保留现有 10 s 护栏） | `cargo test` |
| 契约 | Channel 事件顺序、`Done` 唯一性、取消后无迟到副作用 | `cargo test` + 前端 mock |
| 基准 | 1k / 10k / 50k 冷启动与暖查询分位数 | 独立脚本，手动/定期 |

### 10.2 基准脚本

- 语料生成：`scripts/gen-search-corpus.mjs`，固定随机种子，三档规模；大小分布 70% 2–8 KB、20% 8–32 KB、10% 32–256 KB，含 30% 中文正文与少量 5 MiB 边界文件。
- 采集：`scripts/bench-search.mjs` 调用 `cargo run --release --bin bench_search -- --root <corpus> --query <q> --repeat 30 --warmup 5 --json`，输出 `{ coldMs, warmP50, warmP95, peakRssMb, cachedBytes, hits }`。
- Rust 侧新增 `src-tauri/src/bin/bench_search.rs`；需要把搜索逻辑从 `notes.rs` 抽到可复用的 `src/search.rs`（`pub` API），命令层只做参数转换与事件发送。
- 结果写入 `docs/perf/results/YYYY-MM-DD-<machine>.json`，人工确认后提升为基线。

### 10.3 基线文件

`docs/perf/baseline/<machine-tier>.json`：

```json
{
  "tier": "dev-i9-14900hx",
  "capturedAt": "2026-10-23",
  "machine": { "cpu": "Intel(R) Core(TM) i9-14900HX", "cores": 32, "ramGb": 47.7, "os": "win32 10.0.26300" },
  "toolchain": { "rustc": "rustc 1.96.0", "profile": "release" },
  "query": "needle",
  "results": {
    "1000":  { "notes": 1000,  "bytes": 28573385,   "coldMs": 415.9,   "warmP50Ms": 415.5,   "warmP95Ms": 428.4,   "runs": 30 },
    "10000": { "notes": 10000, "bytes": 225788729,  "coldMs": 4076.3,  "warmP50Ms": 4050.8,  "warmP95Ms": 4087.4,  "runs": 20 },
    "50000": { "notes": 50000, "bytes": 1138251705, "coldMs": 20193.3, "warmP50Ms": 20279.5, "warmP95Ms": 20526.7, "runs": 10 }
  }
}
```

首轮实测已填入（2026-10-23，改造前）；后续变化按 §3.3 的倍率门禁比较。`peakRssMb` 暂缺，待 L2 增加内存采集。

机器档位命名（§14 Q8）：`dev-<cpu 短名>`、`ci-<runner 名>`；首轮只建 `dev-i9-14900hx` 一档，CI runner 档在手动基准 job 首次跑通后补建。不同档位之间不比较，只与各自历史基线比较。

### 10.4 CI 策略

- 必需检查保持功能测试：`bun run test`、`bun run typecheck`、`bun run build`、`cargo test`、文档门禁。
- 基准**不进**必需检查（§14 Q7）：GitHub runner 抖动会制造假失败，且目前没有稳定机器档位。首版只在本机跑，PR 里附 `docs/perf/results/*.json` 摘要。
- 待同一 runner 连续 5 次波动 <10% 后，再加 `workflow_dispatch` 手动 job；不设夜间定时任务。

## 11. 文档产出物与编号

| 产物 | 路径 | 内容 |
|---|---|---|
| RFC-006 | `docs/rfc/RFC-006-search-performance.md` | REQ-022~030、§1 需求、§8 追溯，沿用现有 RFC 结构 |
| ADR-007 | `docs/adr/007-search-snapshot-cache.md` | 快照策略，Supersedes ADR-004 |
| ADR-008 | `docs/adr/008-streaming-search-channel.md` | 流式契约与取消语义 |
| ADR-009 | `docs/adr/009-workspace-watcher.md` | 监听失效与降级 |
| spec | 本文件 | 需求与决策快照 |
| plan | `docs/plans/2026-10-23-search-performance-plan.md` | 任务清单与验证命令 |
| backlog | `docs/backlog.md` 新增 B4 区块 | B4-1~B4-9 |
| ADR 索引 | `docs/adr/README.md` | ADR-004 改 Superseded，登记 007~009 |
| C1/C2/C3 | `docs/architecture/*` | 新增 Snapshot、Watcher、Channel 组件与数据流；外部依赖加 notify/并行库 |
| README | `md-studio/README.md` | 性能基线、已知限制、命令 |
| 知识库 | `knowledge.md` | 搜索策略、缓存预算、基准命令、门禁口径 |
| 门禁 | `node scripts/check-eng-docs.cjs` | 必须 PASS |

编号现状：下一个 RFC 为 **RFC-006**，下一个 ADR 为 **ADR-007**，下一个 REQ 为 **REQ-022**（由门禁实测得出）。

## 12. Backlog 拆分草案

新增 B4 区块，避免与现有 B1/B2/B3 编号冲突。

| ID | 项 | 优先级 | 收尾标准 |
|---|---|---|---|
| B4-1 | L0 扫描热点优化 | P1 | 1k 档 release 实测比 313 ms 下降 ≥10%；匹配语义测试全绿 |
| B4-2 | L1 并行扫描 | P1 | 1k 档 ≤120 ms（本机），命中集合与串行完全一致 |
| B4-3 | L2 内存快照 + LRU | P0 | 10k 档暖查询 p95 ≤20 ms；256 MB 预算下淘汰正确；降级路径正确 |
| B4-4 | 后台预热与状态 | P0 | 打开工作区不阻塞；`Warming/Ready/Degraded` 状态可见 |
| B4-5 | 流式契约（Channel） | P0 | 旧命令退役；前端逐批渲染；取消无迟到副作用 |
| B4-6 | 文件监听 + mtime 兜底 | P1 | 外部编辑 500 ms 内可见；监听失败自动降级 |
| B4-7 | MAX_NOTES 50k + 侧栏虚拟化 | P2 | 50k 工作区可打开、可搜、树不卡；键盘/无障碍不回归 |
| B4-8 | 基准脚本 + 基线门禁 | P1 | 三档语料可复现；1.2×/2× 阈值可判定 |
| B4-9 | L3 触发评估（ADR-010） | P3 | 触发条件写入 ADR-010 草案，附候选对比维度 |

## 13. REQ 草案（待 RFC-006 定稿）

| REQ | 能力 | 基线 | 目标 | 验证 |
|---|---|---|---|---|
| REQ-022 | L0 扫描热点优化 | release 313 ms @1k | 同语料下降 ≥10%，语义不变 | `cargo test` + bench |
| REQ-023 | L1 并行扫描 | 串行 313 ms | ≤120 ms @1k，结果集一致 | bench + 黄金结果比对 |
| REQ-024 | L2 内存快照与 LRU | 每次查询全量读盘 | 10k 暖查询 p95 ≤20 ms；256 MB 可配置 | bench + 单测 |
| REQ-025 | 后台预热与状态机 | 无预热概念 | 打开工作区后台预热，三态可见，失败降级 | 集成测试 + 手动 |
| REQ-026 | 流式搜索契约 | 一次性 `SearchHit[]` | Channel 逐批返回，`Done` 汇总，可取消 | 契约测试 + App 测试 |
| REQ-027 | 文件监听失效 | 外部改动不可见 | 监听 + mtime 兜底，500 ms 内可见 | 集成测试（临时目录） |
| REQ-028 | 50k 上限与树虚拟化 | 10,000 硬上限，全量 DOM | 50k 可打开可搜索，树虚拟化 | 性能测试 + UI 测试 |
| REQ-029 | 自适应防抖与首屏 | 固定 250 ms | 80/120/200 ms；首屏 p95 ≤100 ms @10k | App 测试 + 手动计时 |
| REQ-030 | 基准与基线门禁 | 单个 10 s 护栏 | 三档基准、分机器基线、1.2×/2× 判定 | 脚本 + 基线文件 |

## 14. 已裁决问题（Q1~Q10）与残余风险

裁决原则：正确性优先于延迟；任何优化都必须可降级；所有指标都必须可重复测量。

| # | 问题 | 裁决 | 落地位置 |
|---|---|---|---|
| Q1 | 「全量缓存」与 LRU 的措辞冲突 | 统一表述为「预算内全量、超预算 LRU」；`全量缓存` 不再单独出现。ADR-007、README、知识库使用同一措辞 | ADR-007、§1、§9.4 |
| Q2 | 50k × 5 MiB 理论上限 50 GB，极端语料持续淘汰 | 验收以真实分布语料为准（中位 4 KB，约 200 MB）；命中率 <80% 或 p95 >50 ms 时改按「不慢于实时扫描」验收并触发 ADR-010，不承诺极端语料 | §3.2、§5 L3 |
| Q3 | Channel 让现有 `invoke` mock 需要模拟事件 | mock `@tauri-apps/api/core` 时一并提供可控 `Channel` 假实现；`App.test.tsx` 驱动流式渲染，`noteRepository.test.ts` 断言事件映射；不新增适配器接口 | §7.2、plan Task 4 |
| Q4 | notify 在 OneDrive/网络盘/原子保存下的可靠性 | 监听为 best-effort：只监听 Markdown 与目录、300 ms 去抖；失败或溢出置 `Degraded`，查询前 mtime+size 校验；手动刷新全量对账；集成测试覆盖 rename 原子保存 | ADR-009、§8 |
| Q5 | 虚拟化引入依赖或自建窗口化的取舍 | 自建固定行高窗口化（24 px、overscan 20、`ResizeObserver`），零新依赖；`viewportHeight` 可注入；保留 `tree/treeitem` 角色与键盘导航 | §9.3、REQ-028 |
| Q6 | 流式先到先显与稳定排序冲突 | 每批到达后对当前 ≤50 条排序再渲染；`Done` 不重排。排序成本可忽略，顺序始终稳定 | §9.2、REQ-026 |
| Q7 | 基准是否进 CI | 不进必需检查；首版本机跑并在 PR 附结果；同一 runner 连续 5 次波动 <10% 后，才加 `workflow_dispatch` 手动 job；不设夜间任务 | §10.4 |
| Q8 | 机器档位如何划分 | 命名 `dev-<cpu>` / `ci-<runner>`；首轮只建 `dev-i9-14900hx`，CI 档跑通后补建；跨档不比较，只与各自历史比 | §10.3 |
| Q9 | ADR-003「不缓存正文」措辞 | 不重写 ADR-003；在文末追加 Amended 注记，明确「不缓存正文」指不建持久化正文库，进程内只读缓存不改变事实源；ADR-007 引用该注记 | ADR-003、ADR-007 |
| Q10 | 50k 后 `list_notes` 与树构建耗时 | 纳入 REQ-028 验收：先实测 IPC 体积与树构建；打开工作区 >2 s 时引入按目录懒加载（B4-7 子项）。虚拟化只解决渲染，不解决数据体积 | REQ-028、B4-7、plan Task 7 |

残余风险（接受并监控，不再作为开放问题）：

- Windows Defender 实时保护会显著影响冷启动，基线文件必须记录其状态；关闭 Defender 的数字不作为验收依据。
- `notify` 在部分网络盘上完全不工作，此类工作区永久处于 `Degraded`，搜索仍正确但更慢。
- 256 MB 预算在高分辨率长文语料下会频繁淘汰，暖查询可能退化为读盘扫描。
- 50k 条目一次性 IPC 传输约 5–8 MB JSON，首次打开工作区的耗时会明显高于 1k 档。

## 15. 访谈记录

| 轮次 | 议题 | 选择 |
|---|---|---|
| 1 | 文档组合 | 全套 + backlog 拆分 |
| 1 | 覆盖层级 | L0~L3 全部 |
| 1 | 目标规模 | 1k / 10k / 50k |
| 1 | 文档定位 | 承诺性 SLO + 验收 |
| 2 | ADR-004 | 新 ADR 取代 |
| 2 | 缓存范围 | 全量缓存 |
| 2 | 失效策略 | 文件监听 + 手动刷新 |
| 2 | 结果契约 | 流式增量返回 |
| 3 | 缓存安全阀 | 总量上限 + LRU 淘汰 |
| 3 | 基准方式 | cargo test + 独立脚本 |
| 3 | 阈值形式 | 相对基线倍率 |
| 3 | 防抖 | 自适应 |
| 4 | 缓存上限 | 256 MB 可配置 |
| 4 | 基线存储 | 入库 + 分机器档位 |
| 4 | 流式机制 | Tauri Channel 推送 |
| 4 | 监听降级 | 查询时 mtime 校验 |
| 5 | MAX_NOTES | 提高到 50k |
| 5 | 依赖策略 | 不限，按效果选 |
| 5 | ADR 拆分 | 3 份 |
| 5 | spec 位置 | 按项目门禁，放弃 `*-spec.md` 命名 |
| 6 | 预热时机 | 打开工作区即后台预热 |
| 6 | 命令兼容 | 新命令取代旧命令 |
| 6 | 倍率阈值 | 1.2× 中位数 + 2× p95 |
| 6 | spec 形态 | 需求与决策快照 |
| 7 | 绝对目标 | 暖查询 p95 ≤20 ms @10k |
| 7 | 侧栏渲染 | 需要虚拟化 |
| 7 | L3 定位 | 直接列为未来 ADR |
| 8 | 开放问题 Q1~Q10 | 全部裁决，见 §14 |

## 16. 下一步

1. §14 的 Q1~Q10 已裁决，不再作为评审阻塞项；推翻裁决需在 RFC-006 评审记录中写明理由。
2. 按 §11 建 RFC-006 与 ADR-007~009，更新 ADR 索引与 C3。
3. 拆实现计划：先 L0 + 基准脚本（可独立验证），再 L2 + 流式，最后监听、50k 与虚拟化。
4. 首轮实测后回填 SLO 与基线文件，启用回归门禁。
5. 只有 §5 L3 的触发条件成立时，才创建 ADR-010。

本轮未改任何代码，也未修改其他文档；新增文件仅为本文档。
