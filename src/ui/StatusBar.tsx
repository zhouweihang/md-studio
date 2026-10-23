import { countLines, countWords } from '../core/notes'

export type SaveState = 'idle' | 'pending' | 'saved' | 'error' | 'conflict'

interface StatusBarProps {
  path: string
  content: string
  dirty: boolean
  state: SaveState
}

const labels: Record<SaveState, string> = {
  idle: '已就绪',
  pending: '未保存',
  saved: '已保存',
  error: '保存失败',
  conflict: '磁盘已变更',
}

export function StatusBar({ path, content, dirty, state }: StatusBarProps) {
  const visualState: SaveState =
    state === 'conflict' || state === 'error' ? state : dirty ? 'pending' : state

  return (
    <footer className="status-bar">
      <span className={`save-state state-${visualState}`}>
        <span className="status-dot" aria-hidden="true" />
        {labels[visualState]}
      </span>
      <span className="status-separator" aria-hidden="true" />
      <span>{countWords(content)} 字</span>
      <span>{countLines(content)} 行</span>
      <span className="status-path" title={path}>
        {path}
      </span>
    </footer>
  )
}
