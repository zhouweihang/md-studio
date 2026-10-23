import { useCallback, useEffect, useRef, useState } from 'react'
import { WorkspaceService } from '../application/workspace'
import { TauriNoteRepository } from '../adapters/tauri/noteRepository'
import { pickWorkspace } from '../adapters/tauri/picker'
import { DebouncedSaver } from '../core/autosave'
import type { NoteEntry } from '../core/notes'
import { EditorPane } from './EditorPane'
import { PreviewPane } from './PreviewPane'
import { Sidebar } from './Sidebar'
import { StatusBar, type SaveState } from './StatusBar'

const service = new WorkspaceService(new TauriNoteRepository())
const STORAGE_KEY = 'mdstudio.workspace'

type ViewMode = 'split' | 'edit' | 'preview'

interface OpenedNote {
  path: string
  content: string
  savedContent: string
}

function describeError(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return String(error)
}

function friendlyError(error: unknown): string {
  const message = describeError(error)
  if (message.includes('escapes')) return '路径越界，已拒绝访问。'
  if (message.includes('too large')) return '文件超过 5 MiB 限制。'
  if (message.includes('UTF-8')) return '文件不是有效的 UTF-8 文本。'
  if (message.includes('only Markdown')) return '只支持 Markdown 文件。'
  if (message.includes('cannot open workspace')) return '无法打开工作区，请检查文件夹是否存在。'
  return message
}

function readStoredWorkspace(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY)
  } catch {
    return null
  }
}

function storeWorkspace(path: string | null): void {
  try {
    if (path) localStorage.setItem(STORAGE_KEY, path)
    else localStorage.removeItem(STORAGE_KEY)
  } catch {
    // Storage is a convenience, not a requirement.
  }
}

function EmptyState({
  root,
  noteCount,
  loading,
  onOpen,
}: {
  root: string | null
  noteCount: number
  loading: boolean
  onOpen: () => void
}) {
  return (
    <div className="empty-state">
      <span className="empty-kicker">LOCAL-FIRST MARKDOWN DESK</span>
      <h1>{root ? '选一篇笔记开始' : '把文件夹变成写作工作区'}</h1>
      <p>
        {root
          ? `已载入 ${noteCount} 篇 Markdown。左侧选择文件，右侧直接编辑；修改会在 700ms 后自动保存。`
          : '选择一个本地文件夹，MD Studio 会递归找出其中的 Markdown 文件。内容始终留在你的磁盘上。'}
      </p>
      <button type="button" className="button primary large" onClick={onOpen} disabled={loading}>
        {loading ? '读取中…' : root ? '切换文件夹' : '打开 Markdown 文件夹'}
      </button>
      <ul className="empty-points">
        <li>纯本地文件，兼容 Typora、VS Code 与 Git</li>
        <li>实时 GFM 预览，危险 HTML 会被清理</li>
        <li>自动保存 + Ctrl+S，路径越界由 Rust 侧拦截</li>
      </ul>
    </div>
  )
}

export function App() {
  const [root, setRoot] = useState<string | null>(null)
  const [notes, setNotes] = useState<NoteEntry[]>([])
  const [opened, setOpened] = useState<OpenedNote | null>(null)
  const [query, setQuery] = useState('')
  const [viewMode, setViewMode] = useState<ViewMode>('split')
  const [loading, setLoading] = useState(false)
  const [saveState, setSaveState] = useState<SaveState>('idle')
  const [error, setError] = useState<string | null>(null)
  const saverRef = useRef<DebouncedSaver | null>(null)

  const dirty = opened ? opened.content !== opened.savedContent : false

  const showError = useCallback((next: string) => {
    setError(next)
  }, [])

  const flushCurrent = useCallback(async (): Promise<boolean> => {
    const saver = saverRef.current
    if (!saver) return true

    try {
      await saver.flush()
      setSaveState('saved')
      return true
    } catch (caught) {
      setError(friendlyError(caught))
      setSaveState('error')
      return false
    }
  }, [])

  const loadWorkspace = useCallback(async (path: string) => {
    setLoading(true)
    try {
      const entries = await service.list(path)
      saverRef.current?.dispose()
      saverRef.current = null
      setRoot(path)
      setNotes(entries)
      setOpened(null)
      setQuery('')
      setSaveState('idle')
      setError(null)
      storeWorkspace(path)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    const stored = readStoredWorkspace()
    if (!stored) return

    void loadWorkspace(stored).catch((caught: unknown) => {
      storeWorkspace(null)
      setRoot(null)
      setNotes([])
      setError(friendlyError(caught))
    })
  }, [loadWorkspace])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
        event.preventDefault()
        void flushCurrent()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [flushCurrent])

  useEffect(() => {
    const name = opened?.path.split('/').at(-1)
    document.title = name ? `${dirty ? '• ' : ''}${name} — MD Studio` : 'MD Studio'
  }, [dirty, opened?.path])

  const handleOpenWorkspace = useCallback(async () => {
    if (!(await flushCurrent())) return

    try {
      const path = await pickWorkspace()
      if (!path) return
      await loadWorkspace(path)
    } catch (caught) {
      setError(friendlyError(caught))
    }
  }, [flushCurrent, loadWorkspace])

  const handleRefresh = useCallback(async () => {
    if (!root) return
    setLoading(true)
    try {
      setNotes(await service.list(root))
      setError(null)
    } catch (caught) {
      setError(friendlyError(caught))
    } finally {
      setLoading(false)
    }
  }, [root])

  const openNote = useCallback(
    async (note: NoteEntry) => {
      if (!root || opened?.path === note.path) return
      if (opened && !(await flushCurrent())) return

      try {
        const content = await service.read(root, note.path)
        const pathAtOpen = note.path

        const saver = new DebouncedSaver(700, async (text) => {
          try {
            await service.save(root, pathAtOpen, text)
            setOpened((current) =>
              current && current.path === pathAtOpen ? { ...current, savedContent: text } : current,
            )
            setSaveState('saved')
          } catch (caught) {
            setSaveState('error')
            setError(`保存失败：${friendlyError(caught)}`)
            throw caught
          }
        })

        saverRef.current?.dispose()
        saverRef.current = saver
        setOpened({ path: note.path, content, savedContent: content })
        setSaveState('idle')
        setError(null)
      } catch (caught) {
        setError(friendlyError(caught))
      }
    },
    [flushCurrent, opened?.path, root],
  )

  const handleChange = useCallback((content: string) => {
    setOpened((current) => (current ? { ...current, content } : current))
    setSaveState('pending')
    saverRef.current?.schedule(content)
  }, [])

  const handleSave = useCallback(() => {
    void flushCurrent()
  }, [flushCurrent])

  const handleCreate = useCallback(
    async (title: string) => {
      if (!root) return
      try {
        const note = await service.create(root, title)
        setNotes(await service.list(root))
        await openNote(note)
      } catch (caught) {
        setError(friendlyError(caught))
      }
    },
    [openNote, root],
  )

  return (
    <div className="app">
      {error && (
        <div className="error-bar" role="alert">
          <span>{error}</span>
          <button type="button" onClick={() => setError(null)} aria-label="关闭错误提示">
            ×
          </button>
        </div>
      )}

      <div className="app-body">
        <Sidebar
          root={root}
          notes={notes}
          activePath={opened?.path ?? null}
          query={query}
          loading={loading}
          onQueryChange={setQuery}
          onOpenWorkspace={() => void handleOpenWorkspace()}
          onRefresh={() => void handleRefresh()}
          onSelect={(note) => void openNote(note)}
          onCreate={(title) => void handleCreate(title)}
        />

        <main className="workspace">
          {opened ? (
            <>
              <header className="doc-toolbar">
                <div className="doc-title">
                  <span className="doc-name">{opened.path.split('/').at(-1)}</span>
                  {dirty && <span className="dirty-dot" title="有未保存的修改" />}
                </div>

                <div className="view-switch" role="group" aria-label="视图模式">
                  {(
                    [
                      ['split', '分栏'],
                      ['edit', '编辑'],
                      ['preview', '预览'],
                    ] as const
                  ).map(([mode, label]) => (
                    <button
                      key={mode}
                      type="button"
                      className={viewMode === mode ? 'is-active' : ''}
                      aria-pressed={viewMode === mode}
                      onClick={() => setViewMode(mode)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </header>

              <div className={`doc-panes mode-${viewMode}`}>
                {viewMode !== 'preview' && (
                  <section className="pane editor-pane" aria-label="Markdown 源文本">
                    <EditorPane value={opened.content} onChange={handleChange} onSave={handleSave} />
                  </section>
                )}
                {viewMode !== 'edit' && (
                  <section className="pane preview-pane" aria-label="Markdown 预览">
                    <PreviewPane content={opened.content} onError={showError} />
                  </section>
                )}
              </div>

              <StatusBar path={opened.path} content={opened.content} dirty={dirty} state={saveState} />
            </>
          ) : (
            <EmptyState
              root={root}
              noteCount={notes.length}
              loading={loading}
              onOpen={() => void handleOpenWorkspace()}
            />
          )}
        </main>
      </div>
    </div>
  )
}
