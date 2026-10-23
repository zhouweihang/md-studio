import { useEffect, useState, type FormEvent } from 'react'

export type NoteDialogMode = 'rename' | 'delete'

interface NoteDialogProps {
  mode: NoteDialogMode
  fileName: string
  initialTitle: string
  busy: boolean
  error: string | null
  onCancel: () => void
  onRename: (title: string) => void
  onDelete: () => void
}

export function NoteDialog({
  mode,
  fileName,
  initialTitle,
  busy,
  error,
  onCancel,
  onRename,
  onDelete,
}: NoteDialogProps) {
  const [title, setTitle] = useState(initialTitle)
  const isRename = mode === 'rename'

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busy) onCancel()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [busy, onCancel])

  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (busy) return
    if (isRename) {
      const value = title.trim()
      if (value) onRename(value)
    } else {
      onDelete()
    }
  }

  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onCancel()
      }}
    >
      <div className="dialog" role="dialog" aria-modal="true" aria-labelledby="note-dialog-title">
        <form onSubmit={submit}>
          <span className="dialog-kicker">{isRename ? 'RENAME NOTE' : 'DELETE NOTE'}</span>
          <h2 id="note-dialog-title">{isRename ? '重命名笔记' : '删除笔记'}</h2>

          {isRename ? (
            <>
              <label htmlFor="note-dialog-name">新名称</label>
              <input
                id="note-dialog-name"
                autoFocus
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                disabled={busy}
                spellCheck={false}
              />
              <p className="dialog-hint">扩展名与所在目录保持不变。当前文件：{fileName}</p>
            </>
          ) : (
            <p className="dialog-body">
              确定删除 <strong>{fileName}</strong>？文件会移入系统回收站，删除后可立即撤销。
            </p>
          )}

          {error && (
            <p className="dialog-error" role="alert">
              {error}
            </p>
          )}

          <div className="dialog-actions">
            <button type="button" className="button outline" onClick={onCancel} disabled={busy}>
              取消
            </button>
            <button
              type="submit"
              autoFocus={!isRename}
              className={`button ${isRename ? 'primary' : 'danger'}`}
              disabled={busy || (isRename && !title.trim())}
            >
              {busy ? '处理中…' : isRename ? '重命名' : '删除'}
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}
