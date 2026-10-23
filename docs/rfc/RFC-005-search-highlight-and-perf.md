# RFC-005: 全文搜索命中高亮与大工作区性能基线（域 Search）

> Status: Implemented (Completed 2026-10-23) | Type: RFC | Created: 2026-10-23 | Updated: 2026-10-23 | Authors: Buffy | Reviewers: 项目所有者 | Target: v0.5.0
> 索引：`RFC-005` | 关联 REQ：REQ-020~REQ-021 | 收益：搜索结果里一眼看到命中的词；大工作区扫描性能有可重复的回归基线。
> 位置：`docs/rfc/RFC-005-search-highlight-and-perf.md` 权威；`docs/specs/2026-10-23-search-highlight-and-perf-design.md` 为实现快照
> 一致性声明：本文档与 RFC-002（全文搜索）、ADR-004（Rust 实时扫描，不建索引）保持一致；不改变搜索命令契约，不引入全文索引或虚拟列表。

## 0. 更新摘要

2026-10-23：建立 RFC-005。RFC-002 的内容命中只渲染纯文本片段，用户需要在片段里自己找关键词；同时搜索只有功能测试，没有覆盖大工作区的耗时基线。本 RFC 在前端增加纯函数高亮，在 Rust 增加 1,000 篇笔记的搜索耗时回归测试。

## 1. 需求

| REQ | 能力 | 基线 | 目标 | 验证 |
|---|---|---|---|---|
| REQ-020 | 搜索命中高亮 | 0：`SearchHit.preview` 以纯文本渲染（`src/ui/Sidebar.tsx`） | 内容命中片段中所有大小写不敏感的匹配用 `<mark>` 包住；无匹配时原样渲染 | `cd md-studio && bun run test highlight` |
| REQ-021 | 大工作区性能测试 | 0：搜索只有功能用例（Rust 15→现有 21 个测试），无耗时断言 | 1,000 篇笔记 + 1 篇命中时，`search_notes_blocking` 返回正确命中且耗时低于 10s；测试输出实测耗时 | `cd md-studio/src-tauri && cargo test large_workspace` |

## 2. 现状根因

- `src/core/search.ts` 只有 `SearchHit` 类型；`Sidebar` 直接把 `hit.preview` 放进 `<span>`，没有强调命中位置。
- `build_preview` 已经围绕命中词截取片段，因此片段中必然包含至少一次查询词；前端可以安全地按查询词切分。
- Rust 搜索测试只断言命中数量、行号与片段内容，没有构造大目录，也没有时间预算，性能退化不会被发现。
- ADR-004 明确「实时扫描、不建索引」，因此性能保护必须来自可重复的耗时测试，而不是索引。

## 3. 总体设计

```mermaid
flowchart LR
  Query[搜索框 query] --> Sidebar[Sidebar]
  Hits[SearchHit.preview] --> Sidebar
  Sidebar -->|highlightParts(preview, query)| Core[src/core/search.ts]
  Core -->|HighlightPart[]| Sidebar
  Sidebar -->|mark 标记| UI[内容命中列表]
  RustTest[cargo test] -->|1,000 文件 + 耗时断言| Search[search_notes_blocking]
```

不变量：

1. 高亮是纯展示逻辑，放在 `src/core`，不 import React/DOM/Tauri。
2. 切分只改变渲染节点，不改变片段文本；拼接所有 part 必须等于原 preview。
3. 搜索命令契约不变：仍返回 `SearchHit[]`，不新增偏移字段。
4. 性能测试使用真实文件系统与 `search_notes_blocking`，阈值宽松但可发现数量级退化。

## 4. 详细设计

### 4.1 高亮纯函数（REQ-020）

```ts
export interface HighlightPart {
  text: string
  match: boolean
}

export function highlightParts(text: string, query: string): HighlightPart[]
```

- `query.trim()` 为空 → 单个 `{ text, match: false }`。
- 大小写不敏感：对 `text` 与 query 都做 `toLocaleLowerCase()` 后查找；输出使用原文切片，保留原始大小写。
- 循环查找所有不重叠匹配；无匹配时返回单个未命中 part。
- `Sidebar` 用 `<mark className="hit-mark">` 渲染命中段；CSS 使用侧栏的暖色强调，不引入新颜色变量。

### 4.2 大工作区性能测试（REQ-021）

Rust `#[test] fn search_notes_handles_a_large_workspace_within_budget()`：

1. 在临时目录创建 `bulk/note-0000.md` … `note-0999.md`，每篇约 300 字节、含多行正文。
2. 追加 `bulk/target.md`，正文包含唯一关键词 `needle`。
3. `Instant::now()` → `search_notes_blocking(root, "needle")` → `elapsed()`。
4. 断言恰好 1 条命中、路径为 `bulk/target.md`、耗时 < 10s；`println!` 输出实测耗时，便于本地对比。
5. 阈值取 10s 是回归护栏（正常机器远低于 1s），不是性能 SLA；避免 CI 抖动导致假失败。

### 4.3 边界

- 高亮只处理搜索结果片段，不在编辑器 textarea 内做富文本高亮（textarea 无法局部着色）。
- 大小写折叠可能改变个别 Unicode 字符长度；以实际 `toLocaleLowerCase()` 的索引切片为准，不做字形归并。
- 性能测试不纳入常规的 CI 基准阈值比较；仅断言不出现数量级退化。

## 5. 错误处理与可观测性

| 场景 | 处理 |
|---|---|
| query 为空/纯空白 | 不高亮，原样渲染 |
| preview 中无匹配（理论上不应出现） | 原样渲染，不抛错 |
| 多个匹配 | 全部高亮，保留大小写 |
| 大工作区测试超时 | 断言失败并输出实测耗时，定位搜索退化 |

不引入遥测；测试输出与 UI 是唯一的可观测面。

## 6. 验收

| REQ | 验收结果 |
|---|---|
| REQ-020 | **通过 2026-10-23**：`search.test.ts` 覆盖大小写、多次匹配、中文、空 query、无匹配与拼接还原；`App.test.tsx`「highlights the query inside content search hits」验证以 `AutoSave` 查询时渲染 `<mark>autosave</mark>` |
| REQ-021 | **通过 2026-10-23**：`search_notes_handles_a_large_workspace_within_budget` 在 1,001 篇笔记中命中 1 条，实测 **330.97ms**（debug，Windows），断言 < 10s 并打印耗时 |

## 7. 非目标

- 编辑器内命中的富文本高亮/跳转后选中词：textarea 不支持局部样式，需要换编辑器方案。
- 全文索引、增量索引：ADR-004 已明确不建索引。
- 前端虚拟列表/分页：命中上限 50 条，当前无需虚拟化。
- Criterion 基准与 CI 时间序列：本项目保持 `cargo test` 单轨，避免新增基准设施。

## 8. 追溯

- 实现：`src/core/search.ts`（`HighlightPart`、`highlightParts`）、`src/ui/Sidebar.tsx`（`<mark className="hit-mark">`）、`src/ui/app.css`、`src-tauri/src/notes.rs`（大工作区测试）。
- 测试：前端 36 passed（新增 5 个高亮纯函数用例 + 1 个 App 集成用例）；Rust 22 passed（新增 1 个大工作区性能用例，1,001 篇实测 330.97ms）。
- 命令：`bun run typecheck`、`bun run test`、`bun run build`、`cargo test --locked --all-targets`、`bun run tauri build`（NSIS 1.52 MiB）、`node scripts/check-eng-docs.cjs` 全部通过。
- 实现快照：`docs/specs/2026-10-23-search-highlight-and-perf-design.md`；计划：`docs/plans/2026-10-23-search-highlight-and-perf-plan.md`。
- 本次未创建提交（用户未要求）。
