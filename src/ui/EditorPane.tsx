import type { KeyboardEvent } from 'react'

interface EditorPaneProps {
  value: string
  onChange: (value: string) => void
  onSave: () => void
}

export function EditorPane({ value, onChange, onSave }: EditorPaneProps) {
  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    const textarea = event.currentTarget

    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
      event.preventDefault()
      onSave()
      return
    }

    if (event.key === 'Tab') {
      event.preventDefault()
      const start = textarea.selectionStart
      const end = textarea.selectionEnd
      const next = `${value.slice(0, start)}  ${value.slice(end)}`
      onChange(next)
      requestAnimationFrame(() => {
        textarea.selectionStart = start + 2
        textarea.selectionEnd = start + 2
      })
    }
  }

  return (
    <textarea
      className="editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
      onKeyDown={handleKeyDown}
      placeholder="# 从这里开始写作"
      spellCheck={false}
      aria-label="Markdown 编辑器"
    />
  )
}
