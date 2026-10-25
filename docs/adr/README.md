# ADR 索引

> Updated: 2026-10-25 | 下一份可用编号：ADR-011（ADR-010 预留给 backlog B4-9 的 L3 触发评估）

| 编号 | 决策 | 状态 |
|---|---|---|
| [ADR-001](001-tauri-desktop-shell.md) | 用 Tauri 2 作为桌面外壳 | Accepted |
| [ADR-002](002-ports-and-adapters.md) | 核心域与平台适配器分离 | Accepted |
| [ADR-003](003-filesystem-as-source-of-truth.md) | 本地文件系统是唯一事实源 | Accepted |
| [ADR-004](004-rust-scan-search.md) | 全文搜索用 Rust 实时扫描，不建索引 | Superseded by ADR-007 |
| [ADR-005](005-content-revision-concurrency.md) | 用内容 revision 做乐观并发，不使用文件锁 | Accepted |
| [ADR-006](006-system-trash-and-snapshot-undo.md) | 删除使用系统回收站，撤销使用会话内内容快照 | Accepted |
| [ADR-007](007-search-snapshot-cache.md) | 搜索暖路径使用内存快照缓存（取代 ADR-004） | Accepted |
| [ADR-008](008-streaming-search-channel.md) | 搜索改为 Channel 流式契约 | Accepted |
| [ADR-009](009-workspace-watcher.md) | 用文件监听维护搜索快照失效 | Accepted |
