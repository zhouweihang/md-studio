import { useEffect, useRef, type KeyboardEvent } from 'react'

interface EditorPaneProps {
  path: string
  value: string
  focusLine?: number
  onChange: (value: string) => void
  onSave: () => void
}

export function EditorPane({ path, value, focusLine, onChange, onSave }: EditorPaneProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea || !focusLine) return

    const lines = value.split('\n')
    const index = Math.min(Math.max(focusLine - 1, 0), lines.length - 1)
    let offset = 0
    for (let line = 0; line < index; line += 1) {
      offset += lines[line].length + 1
    }
    const lineText = lines[index].replace(/\r$/, '')
    const end = offset + lineText.length

    textarea.focus()
    textarea.setSelectionRange(offset, end)
    textarea.scrollTop = Math.max(0, (index - 4) * 26)
    // `value` is intentionally read only when the opened note or target line changes.
  }, [path, focusLine])

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
      ref={textareaRef}
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
