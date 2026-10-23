import { useMemo, type MouseEvent } from 'react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { renderMarkdown } from '../adapters/markdown/render'

interface PreviewPaneProps {
  content: string
  onError: (message: string) => void
}

export function PreviewPane({ content, onError }: PreviewPaneProps) {
  const html = useMemo(() => renderMarkdown(content), [content])

  const handleClick = (event: MouseEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement
    const anchor = target.closest('a')
    if (!anchor) return

    const href = anchor.getAttribute('href') ?? ''
    if (href.startsWith('#')) return

    event.preventDefault()
    if (/^https?:\/\//i.test(href)) {
      void openUrl(href).catch((error: unknown) => {
        onError(`无法打开链接：${error instanceof Error ? error.message : String(error)}`)
      })
    }
  }

  return (
    <div className="preview-scroll">
      {content.trim() ? (
        <article className="preview markdown-body" onClick={handleClick} dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <p className="preview-empty">预览会随着输入实时更新。</p>
      )}
    </div>
  )
}
