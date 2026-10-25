import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type UIEvent,
} from 'react'
import { filterNotes, type NoteEntry } from '../core/notes'
import { highlightParts, type SearchHit, type SearchSource, type SearchSummary } from '../core/search'
import { buildNoteTree, directoryPaths, flattenTree } from '../core/tree'

interface SidebarProps {
  root: string | null
  notes: NoteEntry[]
  activePath: string | null
  query: string
  loading: boolean
  contentHits: SearchHit[]
  searching: boolean
  summary: SearchSummary | null
  onQueryChange: (query: string) => void
  onOpenWorkspace: () => void
  onRefresh: () => void
  onSelect: (note: NoteEntry) => void
  onOpenHit: (hit: SearchHit) => void
  onCreate: (title: string) => void
  /** Overrides the measured scroll viewport; tests use this (RFC-006 §4.7). */
  viewportHeight?: number
}

const SOURCE_LABEL: Record<SearchSource, string> = {
  snapshot: '内存快照',
  disk: '实时扫描',
  mixed: '部分快照',
}

/** Fixed row height is what makes windowing possible without measuring text. */
export const ROW_HEIGHT = 24
/** Rows painted above and below the viewport to avoid blank flashes. */
export const OVERSCAN = 20

function workspaceName(root: string): string {
  const parts = root.split(/[\\/]/).filter(Boolean)
  return parts.at(-1) ?? root
}

interface TreeViewProps {
  rows: ReturnType<typeof flattenTree>
  expanded: ReadonlySet<string>
  activePath: string | null
  viewportHeight: number
  scrollTop: number
  onScrollTo: (top: number) => void
  onToggle: (path: string) => void
  onSelect: (note: NoteEntry) => void
}

/**
 * Windowed file tree (RFC-006 §4.7): only the rows inside the viewport plus
 * overscan are in the DOM, so a 50k-note workspace paints ~50 rows instead of
 * 50,000. Directories are rows too, so collapsing a folder drops its subtree
 * from the row list entirely.
 */
function TreeView({
  rows,
  expanded,
  activePath,
  viewportHeight,
  scrollTop,
  onScrollTo,
  onToggle,
  onSelect,
}: TreeViewProps) {
  const listRef = useRef<HTMLUListElement>(null)
  const pendingFocus = useRef(false)
  const [activeIndex, setActiveIndex] = useState(0)
  const rowHeight = ROW_HEIGHT

  useEffect(() => {
    setActiveIndex((index) => Math.min(index, Math.max(0, rows.length - 1)))
  }, [rows.length])

  useEffect(() => {
    if (!pendingFocus.current) return
    pendingFocus.current = false
    const target = listRef.current?.querySelector<HTMLElement>(`[data-index="${activeIndex}"]`)
    target?.focus()
  }, [activeIndex])

  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN)
  const covered = Math.ceil(Math.max(viewportHeight, 0) / rowHeight) + OVERSCAN * 2
  // Without a measured viewport (no layout, e.g. jsdom) still paint a screenful.
  const painted = Math.max(covered, OVERSCAN * 2)
  const last = Math.min(rows.length, first + painted)
  const visible = rows.slice(first, last)

  const focusRow = (index: number) => {
    const top = index * rowHeight
    const outside = top < scrollTop || top + rowHeight > scrollTop + viewportHeight
    if (outside) {
      const centred = Math.max(0, top - viewportHeight / 2)
      const maxTop = Math.max(0, rows.length * rowHeight - viewportHeight)
      onScrollTo(Math.min(centred, maxTop))
    }
    pendingFocus.current = true
    setActiveIndex(index)
  }

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const row = rows[index]
    if (!row) return

    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        if (index + 1 < rows.length) focusRow(index + 1)
        break
      case 'ArrowUp':
        event.preventDefault()
        if (index > 0) focusRow(index - 1)
        break
      case 'ArrowRight':
        event.preventDefault()
        if (row.node.type !== 'dir') break
        if (!expanded.has(row.node.path)) onToggle(row.node.path)
        else if (rows[index + 1]?.depth > row.depth) focusRow(index + 1)
        break
      case 'ArrowLeft': {
        event.preventDefault()
        if (row.node.type === 'dir' && expanded.has(row.node.path)) {
          onToggle(row.node.path)
          break
        }
        for (let candidate = index - 1; candidate >= 0; candidate -= 1) {
          if (rows[candidate].depth < row.depth) {
            focusRow(candidate)
            break
          }
        }
        break
      }
      case 'Enter':
      case ' ':
        event.preventDefault()
        if (row.node.type === 'dir') onToggle(row.node.path)
        else onSelect(row.node.note)
        break
      default:
    }
  }

  return (
    <ul role="tree" aria-label="笔记文件树" ref={listRef} className="tree">
      {visible.map((row, offset) => {
        const index = first + offset
        const node = row.node
        const isDir = node.type === 'dir'
        const open = isDir && expanded.has(node.path)
        const selected = index === activeIndex

        return (
          <li
            key={node.path}
            role="treeitem"
            aria-level={row.depth + 1}
            aria-expanded={isDir ? open : undefined}
            aria-selected={selected}
            aria-current={!isDir && node.path === activePath ? 'page' : undefined}
            className={isDir ? 'tree-dir-row' : 'tree-file-row'}
            style={{
              position: 'absolute',
              top: index * rowHeight,
              left: 0,
              right: 0,
              height: rowHeight,
            }}
          >
            <button
              type="button"
              className={
                isDir
                  ? `tree-row tree-dir ${open ? 'is-open' : ''}`
                  : `tree-row tree-file ${node.path === activePath ? 'is-active' : ''}`
              }
              style={{ paddingLeft: 8 + row.depth * 12 }}
              tabIndex={selected ? 0 : -1}
              data-index={index}
              title={node.path}
              onClick={() => (isDir ? onToggle(node.path) : onSelect(node.note))}
              onFocus={() => setActiveIndex(index)}
              onKeyDown={(event) => handleKeyDown(event, index)}
            >
              {isDir ? (
                <>
                  <span className="tree-caret" aria-hidden="true" />
                  <span className="tree-dir-name">{node.name}</span>
                </>
              ) : (
                <>
                  <span className="tree-file-mark" aria-hidden="true">
                    #
                  </span>
                  <span className="tree-file-name">{node.name}</span>
                </>
              )}
            </button>
          </li>
        )
      })}
    </ul>
  )
}

export function Sidebar({
  root,
  notes,
  activePath,
  query,
  loading,
  contentHits,
  searching,
  summary,
  onQueryChange,
  onOpenWorkspace,
  onRefresh,
  onSelect,
  onOpenHit,
  onCreate,
  viewportHeight,
}: SidebarProps) {
  const [title, setTitle] = useState('')
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set())
  const [scrollTop, setScrollTop] = useState(0)
  const [measuredViewport, setMeasuredViewport] = useState(0)
  const scrollRef = useRef<HTMLDivElement>(null)
  const treeRef = useRef<HTMLDivElement>(null)

  const filtered = useMemo(() => filterNotes(notes, query), [notes, query])
  const tree = useMemo(() => buildNoteTree(filtered), [filtered])
  const hasQuery = query.trim().length > 0

  // A search only contains matching notes, so reveal every folder that holds a
  // match; outside search the user's collapse state is respected.
  const shownExpanded = useMemo(
    () => (hasQuery ? new Set(directoryPaths(tree)) : expanded),
    [expanded, hasQuery, tree],
  )
  const rows = useMemo(() => flattenTree(tree, shownExpanded), [shownExpanded, tree])

  // Row offset inside the scroll container, so the window works even with the
  // search status above the tree.
  const treeOffset = () => (viewportHeight !== undefined ? 0 : (treeRef.current?.offsetTop ?? 0))

  useEffect(() => {
    if (viewportHeight !== undefined) return
    const element = scrollRef.current
    if (!element || typeof ResizeObserver === 'undefined') return

    const observer = new ResizeObserver((entries) => {
      setMeasuredViewport(entries[0]?.contentRect.height ?? 0)
    })
    observer.observe(element)
    setMeasuredViewport(element.clientHeight)
    return () => observer.disconnect()
  }, [viewportHeight])

  const toggleDir = (path: string) => {
    setExpanded((current) => {
      const next = new Set(current)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }

  const handleScroll = (event: UIEvent<HTMLDivElement>) => {
    const container = event.currentTarget
    setScrollTop(Math.max(0, container.scrollTop - treeOffset()))
  }

  const scrollToTree = (top: number) => {
    const container = scrollRef.current
    if (container) container.scrollTop = treeOffset() + top
    setScrollTop(Math.max(0, top))
  }

  const submit = (event: FormEvent) => {
    event.preventDefault()
    const value = title.trim()
    if (!value) return
    onCreate(value)
    setTitle('')
  }

  const effectiveViewport = viewportHeight ?? measuredViewport

  return (
    <aside className="sidebar">
      <div className="brand">
        <img className="brand-mark" src="/seal.png" alt="" width={38} height={38} />
        <span className="brand-text">
          <strong>MD Studio</strong>
          <small>local markdown desk</small>
        </span>
      </div>

      <div className="workspace-block">
        <span className="eyebrow">工作区</span>
        <div className="workspace-row">
          <div className="workspace-name" title={root ?? undefined}>
            {root ? workspaceName(root) : '尚未打开'}
          </div>
          <button
            type="button"
            className="icon-button"
            onClick={onRefresh}
            disabled={!root || loading}
            title="刷新文件列表"
            aria-label="刷新文件列表"
          >
            ↻
          </button>
        </div>
        <div className="workspace-actions">
          <button type="button" className="button primary" onClick={onOpenWorkspace} disabled={loading}>
            打开文件夹
          </button>
          {root && <span className="workspace-count">{notes.length} 篇</span>}
        </div>
      </div>

      <label className="search">
        <span className="sr-only">搜索笔记</span>
        <input
          type="search"
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder="搜索文件名或正文"
          disabled={!root}
        />
        {query && (
          <button type="button" className="search-clear" onClick={() => onQueryChange('')} aria-label="清除搜索">
            ×
          </button>
        )}
      </label>

      <form className="new-note" onSubmit={submit}>
        <input
          value={title}
          onChange={(event) => setTitle(event.target.value)}
          placeholder="新笔记名称"
          disabled={!root}
          aria-label="新笔记名称"
        />
        <button type="submit" className="button ghost" disabled={!root || !title.trim()}>
          新建
        </button>
      </form>

      <div className="tree-scroll" ref={scrollRef} onScroll={handleScroll}>
        {!root && <p className="sidebar-empty">打开一个文件夹，开始整理你的 Markdown 笔记。</p>}
        {root && (
          <>
            {hasQuery && (
              <div className="search-status" aria-live="polite">
                {searching
                  ? '搜索正文中…'
                  : summary
                    ? `内容命中 ${summary.total} 条 · ${summary.tookMs} ms · ${SOURCE_LABEL[summary.source]}`
                    : `内容命中 ${contentHits.length} 条`}
              </div>
            )}

            {filtered.length > 0 && (
              <section className="tree-section">
                {hasQuery && <div className="section-label">文件名匹配</div>}
                <div
                  className="tree-window"
                  ref={treeRef}
                  style={{ height: rows.length * ROW_HEIGHT }}
                >
                  <TreeView
                    rows={rows}
                    expanded={shownExpanded}
                    activePath={activePath}
                    viewportHeight={effectiveViewport}
                    scrollTop={scrollTop}
                    onScrollTo={scrollToTree}
                    onToggle={toggleDir}
                    onSelect={onSelect}
                  />
                </div>
              </section>
            )}

            {contentHits.length > 0 && (
              <section className="hits-section">
                <div className="section-label">内容匹配</div>
                <ul className="hits">
                  {contentHits.map((hit) => (
                    <li key={`${hit.path}:${hit.line}`}>
                      <button
                        type="button"
                        className="hit"
                        onClick={() => onOpenHit(hit)}
                        title={`${hit.path}:${hit.line}`}
                      >
                        <span className="hit-path">
                          <span className="hit-file">{hit.path}</span>
                          <span className="hit-line">L{hit.line}</span>
                        </span>
                        <span className="hit-preview">
                          {highlightParts(hit.preview, query).map((part, index) =>
                            part.match ? (
                              <mark key={index} className="hit-mark">
                                {part.text}
                              </mark>
                            ) : (
                              <span key={index}>{part.text}</span>
                            ),
                          )}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
                <p className="hits-limit">
                  {summary?.truncated ? '已达 50 条上限，缩小关键词可看到更多' : '最多显示 50 条结果'}
                </p>
              </section>
            )}

            {hasQuery && !searching && filtered.length === 0 && contentHits.length === 0 && (
              <p className="sidebar-empty">没有匹配的笔记或正文。</p>
            )}
            {!hasQuery && filtered.length === 0 && (
              <p className="sidebar-empty">这个工作区还没有 Markdown 文件。</p>
            )}
          </>
        )}
      </div>

      <div className="sidebar-footer">
        <kbd>Ctrl</kbd> + <kbd>S</kbd> 立即保存
      </div>
    </aside>
  )
}
