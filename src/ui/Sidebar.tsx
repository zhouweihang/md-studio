import { useMemo, useState, type FormEvent } from 'react'
import { filterNotes, type NoteEntry } from '../core/notes'
import { highlightParts, type SearchHit, type SearchSource, type SearchSummary } from '../core/search'
import { buildNoteTree, type TreeNode } from '../core/tree'

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
}

const SOURCE_LABEL: Record<SearchSource, string> = {
  snapshot: '内存快照',
  disk: '实时扫描',
  mixed: '部分快照',
}

function workspaceName(root: string): string {
  const parts = root.split(/[\\/]/).filter(Boolean)
  return parts.at(-1) ?? root
}

function TreeBranch({
  nodes,
  activePath,
  onSelect,
}: {
  nodes: TreeNode[]
  activePath: string | null
  onSelect: (note: NoteEntry) => void
}) {
  return (
    <ul className="tree">
      {nodes.map((node) =>
        node.type === 'dir' ? (
          <li key={node.path} className="tree-dir">
            <details open>
              <summary>
                <span className="tree-caret" aria-hidden="true" />
                <span className="tree-dir-name">{node.name}</span>
              </summary>
              <TreeBranch nodes={node.children} activePath={activePath} onSelect={onSelect} />
            </details>
          </li>
        ) : (
          <li key={node.path}>
            <button
              type="button"
              className={`tree-file ${node.path === activePath ? 'is-active' : ''}`}
              aria-current={node.path === activePath ? 'page' : undefined}
              title={node.path}
              onClick={() => onSelect(node.note)}
            >
              <span className="tree-file-mark" aria-hidden="true">
                #
              </span>
              <span className="tree-file-name">{node.name}</span>
            </button>
          </li>
        ),
      )}
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
}: SidebarProps) {
  const [title, setTitle] = useState('')
  const filtered = useMemo(() => filterNotes(notes, query), [notes, query])
  const tree = useMemo(() => buildNoteTree(filtered), [filtered])
  const hasQuery = query.trim().length > 0

  const submit = (event: FormEvent) => {
    event.preventDefault()
    const value = title.trim()
    if (!value) return
    onCreate(value)
    setTitle('')
  }

  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-mark" aria-hidden="true">
          M
        </span>
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
          {root && (
            <span className="workspace-count">
              {notes.length} 篇
            </span>
          )}
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

      <div className="tree-scroll">
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
                <TreeBranch nodes={tree} activePath={activePath} onSelect={onSelect} />
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
