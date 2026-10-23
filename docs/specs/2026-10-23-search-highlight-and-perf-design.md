# 搜索命中高亮与大工作区性能测试 设计快照

> 索引：`RFC-005` | 关联 REQ：REQ-020~REQ-021 | 收益：命中词可见；搜索扫描性能有回归护栏
> 位置：本文档为实现快照；权威见 `docs/rfc/RFC-005-search-highlight-and-perf.md`

## 1. 需求

见 RFC-005 §1。实现范围：`highlightParts` 纯函数、Sidebar `<mark>` 渲染与样式、Rust 1,000 篇笔记搜索耗时测试。不做编辑器内高亮、索引、虚拟列表、基准框架。

## 2. 现状根因

- `src/core/search.ts` 只有 `SearchHit` 类型；Sidebar 把 `hit.preview` 当纯文本渲染。
- Rust 搜索测试只有功能断言，没有大目录与耗时断言。
- `build_preview` 保证片段围绕命中词截取，前端可按 query 安全切分。

## 3. 总体设计

指向 RFC-005 §3；实现落点：

```text
src/core/search.ts           HighlightPart、highlightParts(text, query)
src/core/search.test.ts      高亮纯函数用例
src/ui/Sidebar.tsx           <mark className="hit-mark"> 渲染
src/ui/app.css               .hit-mark 样式
src/ui/App.test.tsx          内容命中渲染 <mark> 的集成用例
src-tauri/src/notes.rs       large workspace 搜索耗时测试
```

## 4. 详细设计（实现视角增量）

### 4.1 高亮

- 实现按 `toLocaleLowerCase()` 查找、按原文切片；切分结果 `map(p => p.text).join('') === text`。
- 空 query / 无匹配 → `[{ text, match: false }]`。
- Sidebar 在 `.hit-preview` 内 map parts：`match ? <mark className="hit-mark"> : <span>`。
- `.hit-mark`：背景 `#6f3826`、文字 `#f7ddcd`、`border-radius: 2px`、`padding: 0 1px`，与侧栏强调色一致。

### 4.2 性能测试

- `search_notes_handles_a_large_workspace_within_budget` 创建 1,000 篇 bulk 笔记（每篇约 300 字节）+ 1 篇命中。
- 计时 `search_notes_blocking`；断言命中 1 条、路径正确、`elapsed < Duration::from_secs(10)`，并打印耗时。
- 测试使用 `tempfile::tempdir`，不接触真实工作区。

### 4.3 测试

- core：英文大小写不敏感、多次匹配、中文、空 query、无匹配、拼接等于原文。
- App：mock `search_notes` 返回命中，输入查询后出现 `<mark>` 且文本为关键词。
- Rust：大工作区耗时断言 + 既有搜索功能用例保持通过。

## 5. Self-Review

- 数据语义：高亮不修改 preview，仅切分渲染；搜索契约不变。
- 与既有机制共存：文件名过滤与内容命中仍并行渲染；性能测试复用 `search_notes_blocking`。
- 已知边界：textarea 内不高亮；性能阈值是回归护栏，不随机器动态校准。
