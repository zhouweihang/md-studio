# ADR-008: 搜索改为 Channel 流式契约

> Status: Accepted | Date: 2026-10-23 | Deciders: 项目所有者 | 关联 RFC: RFC-006 §4.5 | 影响：命令契约、前端端口、测试 mock、取消语义

## Context

现有 `search_notes` 返回一次性 `SearchHit[]`：后端扫完整个工作区才回包，前端在 250 ms 防抖后等待完整结果；切换查询只能丢弃旧响应，无法真正取消后端任务。重构后搜索在内存快照上并行执行，首批结果可以在毫秒级产生，但「等全部扫完再显示」会浪费这段优势；同时 50k 档的扫描时间可能达到数百毫秒，一次性返回会让 UI 出现明显空窗。

约束：

- UI 不得直接调用 Tauri 命令，只有适配器知道命令名（ADR-002）。
- 结果上限 50 条，数据量小，排序与渲染成本可忽略。
- 取消必须能尽快释放线程，避免快速输入时任务排队。
- 现有 App 测试通过 mock `invoke` 驱动，改造不能引入第二套并行的数据通道。

## Decision

1. `search_notes` 改为接收 `tauri::ipc::Channel<SearchEvent>`，流式发送 `Batch { hits }` 与唯一的 `Done { total, truncated, took_ms, source }`。
2. 每批最多 16 条；同一请求的批次按完成顺序发出，前端每批到达后对当前 ≤50 条做稳定排序再渲染，`Done` 不重排。
3. 新增 `request_id`（单调递增）与 `search_cancel(request_id)`；Rust 保存最新请求号，工作线程每批检查一次，过期立即退出。取消不保证零批次，前端按 `request_id` 丢弃迟到事件。
4. **旧的一次性 `search_notes` 退役**：命令、端口、服务、前端与测试全部迁移，不保留双契约。
5. 前端端口改为 `searchNotes(root, query, onBatch): Promise<SearchSummary>`；适配器负责创建 `Channel` 并通过 `invoke` 传入。
6. 测试用 mock 同时提供可控 `Channel` 假实现；适配器单测断言事件映射，App 测试用它驱动渐进渲染。

## Alternatives

- **保留一次性返回，只优化速度**：改动最小，但 50k 档仍有完整扫描的空窗，且无法取消后端任务。
- **分页 `offset/cursor`**：契约简单，但需要多轮往返，首屏延迟被 RPC 次数放大。
- **前端逐文件 `read_note`**：N 次跨进程往返，10k 篇不可接受。
- **本地 HTTP/SSE 或 WebSocket**：能流式，但引入端口、鉴权与生命周期管理，违背 Tauri 的单进程模型。
- **同时保留新旧命令**：兼容性好，但两套契约会长期分叉，测试与文档成本翻倍。

## Consequences

- 正：首批结果在防抖后立即出现，首屏 p95 目标 ≤100 ms 可实现。
- 正：取消真正生效，快速输入不会堆积后端任务。
- 正：`source` 字段让 UI 能显示「快照 / 实时扫描」，便于定位降级。
- 负：所有调用方与测试必须迁移；旧命令无法直接复用。
- 负：流式期间结果顺序按批次到达，需要前端排序兜底；排序在 ≤50 条上成本可忽略。
- 负：Channel 在窗口卸载后写入会失败，后端必须优雅退出而不是 panic。
- 风险：事件顺序与 `Done` 唯一性缺少类型层面的保证，必须用契约测试覆盖。

## References

- RFC-006 §4.5、§5、§6
- ADR-002（端口与适配器）
- ADR-007（内存快照，流式的数据来源）
- `md-studio/docs/specs/2026-10-23-search-performance-design.md`（Q3/Q6 裁决）
- `md-studio/src/adapters/tauri/noteRepository.ts`（实现落点）
