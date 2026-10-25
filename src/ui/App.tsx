import { useCallback, useEffect, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { WorkspaceService } from '../application/workspace'
import { TauriNoteRepository } from '../adapters/tauri/noteRepository'
import { pickWorkspace } from '../adapters/tauri/picker'
import { DebouncedSaver } from '../core/autosave'
import type { DeletedNote, NoteEntry } from '../core/notes'
import { mergeHits, searchDebounceMs } from '../core/search'
import type { SearchHit, SearchSummary, SnapshotStatus } from '../core/search'
import { EditorPane } from './EditorPane'
import { NoteDialog, type NoteDialogMode } from './NoteDialog'
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
  focusLine?: number
}

interface DialogState {
  mode: NoteDialogMode
  title: string
}

function describeError(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return String(error)
}

function friendlyError(error: unknown): string {
  const message = describeError(error)
  if (message.includes('escapes')) return '路径越界，已拒绝访问。'
  if (message.includes('already exists')) return '同名笔记已存在，请换一个名称。'
  if (message.includes('too large')) return '文件超过 5 MiB 限制。'
  if (message.includes('already occupied')) return '原路径已被其他文件占用，无法撤销。'
  if (message.includes('cannot move note to trash')) return '无法移入系统回收站，文件仍在原位置。'
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
  const [contentHits, setContentHits] = useState<SearchHit[]>([])
  const [searching, setSearching] = useState(false)
  const [searchSummary, setSearchSummary] = useState<SearchSummary | null>(null)
  const [viewMode, setViewMode] = useState<ViewMode>('split')
  const [loading, setLoading] = useState(false)
  const [saveState, setSaveState] = useState<SaveState>('idle')
  const [error, setError] = useState<string | null>(null)
  const [dialog, setDialog] = useState<DialogState | null>(null)
  const [dialogBusy, setDialogBusy] = useState(false)
  const [dialogError, setDialogError] = useState<string | null>(null)
  const [undo, setUndo] = useState<DeletedNote | null>(null)
  const saverRef = useRef<DebouncedSaver | null>(null)
  const revisionRef = useRef<string | null>(null)
  const closingRef = useRef(false)
  /** Monotonic search id: the backend cancels anything older (ADR-008). */
  const requestIdRef = useRef(0)
  const snapshotRef = useRef<SnapshotStatus | null>(null)
  /** Mirrors `snapshotRef` for rendering the status bar; the ref feeds debounce. */
  const [snapshot, setSnapshot] = useState<SnapshotStatus | null>(null)
  const noteCountRef = useRef(0)
  noteCountRef.current = notes.length

  const dirty = opened ? opened.content !== opened.savedContent : false
  const conflict = saveState === 'conflict'

  const showError = useCallback((next: string) => {
    setError(next)
  }, [])

  const createSaver = useCallback(
    (workspaceRoot: string, notePath: string) =>
      new DebouncedSaver(700, async (text) => {
        try {
          const expectedRevision = revisionRef.current
          if (!expectedRevision) throw new Error('note revision is missing')

          const nextRevision = await service.save(workspaceRoot, notePath, text, expectedRevision)
          revisionRef.current = nextRevision
          setOpened((current) =>
            current && current.path === notePath ? { ...current, savedContent: text } : current,
          )
          setSaveState('saved')
          setError(null)
        } catch (caught) {
          if (describeError(caught).startsWith('conflict')) {
            setSaveState('conflict')
            setError(null)
          } else {
            setSaveState('error')
            setError(`保存失败：${friendlyError(caught)}`)
          }
          throw caught
        }
      }),
    [],
  )

  const flushCurrent = useCallback(async (): Promise<boolean> => {
    const saver = saverRef.current
    if (!saver) return true

    try {
      await saver.flush()
      setSaveState('saved')
      return true
    } catch (caught) {
      if (describeError(caught).startsWith('conflict')) {
        setSaveState('conflict')
        setError(null)
      } else {
        setError(friendlyError(caught))
        setSaveState('error')
      }
      return false
    }
  }, [])

  const refreshSnapshotStatus = useCallback(async () => {
    let next: SnapshotStatus | null = null
    try {
      next = await service.snapshotStatus()
    } catch {
      // Status is informational; a missing snapshot must never surface as an error.
      next = null
    }
    snapshotRef.current = next
    setSnapshot(next)
  }, [])

  // While the cache is warming, poll until it settles at Ready or Degraded so
  // the status bar shows a live count instead of a stale spinner.
  useEffect(() => {
    if (!root) {
      snapshotRef.current = null
      setSnapshot(null)
      return
    }

    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | undefined

    const poll = async () => {
      let next: SnapshotStatus | null = null
      try {
        next = await service.snapshotStatus()
      } catch {
        next = null
      }
      if (cancelled) return
      snapshotRef.current = next
      setSnapshot(next)
      if (next?.state === 'warming') timer = setTimeout(() => void poll(), 400)
    }

    void poll()
    return () => {
      cancelled = true
      if (timer) clearTimeout(timer)
    }
  }, [root])

  const loadWorkspace = useCallback(async (path: string) => {
    setLoading(true)
    try {
      const entries = await service.list(path)
      saverRef.current?.dispose()
      saverRef.current = null
      revisionRef.current = null
      setRoot(path)
      setNotes(entries)
      void refreshSnapshotStatus()
      setOpened(null)
      setContentHits([])
      setSearchSummary(null)
      setQuery('')
      setUndo(null)
      setSaveState('idle')
      setError(null)
      storeWorkspace(path)
    } finally {
      setLoading(false)
    }
  }, [refreshSnapshotStatus])

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

  useEffect(() => {
    if (!undo) return
    const timer = setTimeout(() => setUndo(null), 8000)
    return () => clearTimeout(timer)
  }, [undo])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    const appWindow = getCurrentWindow()

    void appWindow
      .onCloseRequested(async (event) => {
        if (closingRef.current) return
        event.preventDefault()

        if (!(await flushCurrent())) return

        closingRef.current = true
        try {
          await appWindow.close()
        } catch (caught) {
          closingRef.current = false
          setError(friendlyError(caught))
        }
      })
      .then((stop) => {
        if (disposed) stop()
        else unlisten = stop
      })
      .catch((caught: unknown) => {
        setError(`无法监听关闭事件：${friendlyError(caught)}`)
      })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [flushCurrent])

  useEffect(() => {
    const term = query.trim()
    if (!root || !term) {
      if (requestIdRef.current > 0) void service.cancelSearch(requestIdRef.current)
      setContentHits([])
      setSearchSummary(null)
      setSearching(false)
      return
    }

    // Every keystroke supersedes the previous request: the backend drops stale
    // work and the id guards against batches that arrive late (ADR-008).
    const requestId = requestIdRef.current + 1
    requestIdRef.current = requestId
    let cancelled = false

    setContentHits([])
    setSearchSummary(null)
    setSearching(true)

    const timer = setTimeout(() => {
      void service
        .search(root, term, requestId, (batch) => {
          if (cancelled) return
          setContentHits((current) => mergeHits(current, batch))
        })
        .then((summary) => {
          if (cancelled) return
          setSearchSummary(summary)
          setSearching(false)
        })
        .catch((caught: unknown) => {
          if (cancelled) return
          setContentHits([])
          setSearching(false)
          setError(friendlyError(caught))
        })
    }, searchDebounceMs(noteCountRef.current, snapshotRef.current?.state ?? null))

    return () => {
      cancelled = true
      clearTimeout(timer)
      void service.cancelSearch(requestId)
    }
  }, [query, root])

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
      void refreshSnapshotStatus()
      setError(null)
    } catch (caught) {
      setError(friendlyError(caught))
    } finally {
      setLoading(false)
    }
  }, [refreshSnapshotStatus, root])

  const openNote = useCallback(
    async (note: NoteEntry, focusLine?: number) => {
      if (!root) return

      if (opened?.path === note.path) {
        if (focusLine) {
          setOpened((current) => (current ? { ...current, focusLine } : current))
        }
        return
      }

      if (opened && !(await flushCurrent())) return

      try {
        const document = await service.read(root, note.path)
        revisionRef.current = document.revision
        saverRef.current?.dispose()
        saverRef.current = createSaver(root, note.path)
        setOpened({
          path: note.path,
          content: document.content,
          savedContent: document.content,
          focusLine,
        })
        setSaveState('idle')
        setError(null)
      } catch (caught) {
        setError(friendlyError(caught))
      }
    },
    [createSaver, flushCurrent, opened, root],
  )

  const handleChange = useCallback(
    (content: string) => {
      setOpened((current) => (current ? { ...current, content } : current))
      if (conflict) return
      setSaveState('pending')
      saverRef.current?.schedule(content)
    },
    [conflict],
  )

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

  const handleOpenHit = useCallback(
    (hit: SearchHit) => {
      const note = notes.find((item) => item.path === hit.path)
      if (!note) {
        setError('这篇笔记已被移动或删除，请刷新工作区。')
        return
      }
      void openNote(note, hit.line)
    },
    [notes, openNote],
  )

  const openNoteDialog = useCallback(
    (mode: NoteDialogMode) => {
      if (!opened) return
      const fileName = opened.path.split('/').at(-1) ?? opened.path
      setDialogError(null)
      setDialog({ mode, title: fileName.replace(/\.[^.]+$/, '') })
    },
    [opened],
  )

  const closeDialog = useCallback(() => {
    if (dialogBusy) return
    setDialog(null)
    setDialogError(null)
  }, [dialogBusy])

  const handleRename = useCallback(
    async (title: string) => {
      if (!root || !opened || !dialog || dialog.mode !== 'rename') return

      setDialogBusy(true)
      setDialogError(null)
      let updated: NoteEntry | null = null

      try {
        if (!(await flushCurrent())) {
          setDialogError('当前笔记保存失败，已取消重命名。')
          return
        }

        const renamed = await service.rename(root, opened.path, title)
        updated = renamed
        saverRef.current?.dispose()
        saverRef.current = createSaver(root, renamed.path)
        setOpened((current) =>
          current ? { ...current, path: renamed.path, focusLine: undefined } : current,
        )
        setDialog(null)
        setSaveState('saved')
      } catch (caught) {
        setDialogError(friendlyError(caught))
      } finally {
        setDialogBusy(false)
      }

      if (!updated) return
      try {
        setNotes(await service.list(root))
      } catch (caught) {
        setError(friendlyError(caught))
      }
    },
    [createSaver, dialog, flushCurrent, opened, root],
  )

  const handleDelete = useCallback(async () => {
    if (!root || !opened || !dialog || dialog.mode !== 'delete') return

    setDialogBusy(true)
    setDialogError(null)

    try {
      saverRef.current?.cancel()
      const deleted = await service.delete(root, opened.path)
      saverRef.current?.dispose()
      saverRef.current = null
      revisionRef.current = null
      setOpened(null)
      setUndo(deleted)
      setSaveState('idle')
      setDialog(null)
      setError(null)
    } catch (caught) {
      setDialogError(friendlyError(caught))
      setDialogBusy(false)
      return
    }

    setDialogBusy(false)
    try {
      setNotes(await service.list(root))
    } catch (caught) {
      setError(friendlyError(caught))
    }
  }, [dialog, opened, root])

  const handleUndoDelete = useCallback(async () => {
    if (!root || !undo || undo.content === null) return

    const snapshot = undo
    try {
      const restored = await service.restore(root, snapshot.path, snapshot.content ?? '')
      setNotes(await service.list(root))
      setUndo(null)
      setError(null)
      await openNote(restored)
    } catch (caught) {
      setError(friendlyError(caught))
    }
  }, [openNote, root, undo])

  const reloadFromDisk = useCallback(async () => {
    if (!root || !opened) return

    try {
      const document = await service.read(root, opened.path)
      saverRef.current?.cancel()
      saverRef.current?.dispose()
      revisionRef.current = document.revision
      saverRef.current = createSaver(root, opened.path)
      setOpened((current) =>
        current
          ? {
              ...current,
              content: document.content,
              savedContent: document.content,
              focusLine: undefined,
            }
          : current,
      )
      setSaveState('idle')
      setError(null)
    } catch (caught) {
      setError(friendlyError(caught))
    }
  }, [createSaver, opened, root])

  const overwriteDisk = useCallback(async () => {
    if (!root || !opened) return

    try {
      const latest = await service.read(root, opened.path)
      const localContent = opened.content
      revisionRef.current = latest.revision
      const nextRevision = await service.save(
        root,
        opened.path,
        localContent,
        latest.revision,
      )
      revisionRef.current = nextRevision
      saverRef.current?.cancel()
      setOpened((current) =>
        current && current.path === opened.path
          ? { ...current, savedContent: localContent, focusLine: undefined }
          : current,
      )
      setSaveState('saved')
      setError(null)
    } catch (caught) {
      if (describeError(caught).startsWith('conflict')) {
        setSaveState('conflict')
        setError(null)
      } else {
        setSaveState('error')
        setError(friendlyError(caught))
      }
    }
  }, [opened, root])

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
          contentHits={contentHits}
          searching={searching}
          summary={searchSummary}
          onQueryChange={setQuery}
          onOpenWorkspace={() => void handleOpenWorkspace()}
          onRefresh={() => void handleRefresh()}
          onSelect={(note) => void openNote(note)}
          onOpenHit={handleOpenHit}
          onCreate={(title) => void handleCreate(title)}
        />

        <main className="workspace">
          {conflict && opened && (
            <div className="conflict-banner" role="alert">
              <div>
                <strong>磁盘文件已被外部修改。</strong>
                <span>本地内容尚未覆盖磁盘。</span>
              </div>
              <div className="conflict-actions">
                <button type="button" className="button outline" onClick={() => void reloadFromDisk()}>
                  重新载入
                </button>
                <button type="button" className="button primary" onClick={() => void overwriteDisk()}>
                  覆盖保存
                </button>
              </div>
            </div>
          )}

          {opened ? (
            <>
              <header className="doc-toolbar">
                <div className="doc-title">
                  <span className="doc-name">{opened.path.split('/').at(-1)}</span>
                  {dirty && <span className="dirty-dot" title="有未保存的修改" />}
                </div>

                <div className="doc-toolbar-actions">
                  <div className="doc-actions">
                    <button type="button" className="doc-action" onClick={() => openNoteDialog('rename')}>
                      重命名
                    </button>
                    <button
                      type="button"
                      className="doc-action danger"
                      onClick={() => openNoteDialog('delete')}
                    >
                      删除
                    </button>
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
                </div>
              </header>

              <div className={`doc-panes mode-${viewMode}`}>
                {viewMode !== 'preview' && (
                  <section className="pane editor-pane" aria-label="Markdown 源文本">
                    <EditorPane
                      path={opened.path}
                      value={opened.content}
                      focusLine={opened.focusLine}
                      onChange={handleChange}
                      onSave={handleSave}
                    />
                  </section>
                )}
                {viewMode !== 'edit' && (
                  <section className="pane preview-pane" aria-label="Markdown 预览">
                    <PreviewPane content={opened.content} onError={showError} />
                  </section>
                )}
              </div>

            </>
          ) : (
            <EmptyState
              root={root}
              noteCount={notes.length}
              loading={loading}
              onOpen={() => void handleOpenWorkspace()}
            />
          )}

          <StatusBar
            path={opened?.path ?? null}
            content={opened?.content ?? ''}
            dirty={dirty}
            state={saveState}
            snapshot={snapshot}
          />
        </main>
      </div>

      {undo && (
        <div className="undo-toast" role="status">
          <div className="undo-toast-copy">
            <strong>已移入系统回收站</strong>
            <span>{undo.path}</span>
            {undo.content === null && <span className="undo-note">文件超过 5 MiB 或非 UTF-8，无法在应用内撤销。</span>}
          </div>
          {undo.content !== null && (
            <button type="button" className="undo-action" onClick={() => void handleUndoDelete()}>
              撤销
            </button>
          )}
          <button
            type="button"
            className="undo-dismiss"
            onClick={() => setUndo(null)}
            aria-label="关闭撤销提示"
          >
            ×
          </button>
        </div>
      )}

      {dialog && opened && (
        <NoteDialog
          mode={dialog.mode}
          fileName={opened.path.split('/').at(-1) ?? opened.path}
          initialTitle={dialog.title}
          busy={dialogBusy}
          error={dialogError}
          onCancel={closeDialog}
          onRename={(title) => void handleRename(title)}
          onDelete={() => void handleDelete()}
        />
      )}
    </div>
  )
}
