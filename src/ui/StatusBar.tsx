import { countLines, countWords } from '../core/notes'
import { formatBytes } from '../core/search'
import type { SnapshotState, SnapshotStatus } from '../core/search'

export type SaveState = 'idle' | 'pending' | 'saved' | 'error' | 'conflict'

interface StatusBarProps {
  /** `null` before a note is opened: only workspace-level status is shown. */
  path?: string | null
  content?: string
  dirty?: boolean
  state?: SaveState
  /** Warm-cache lifecycle reported by Rust (RFC-006 §4.4). */
  snapshot?: SnapshotStatus | null
}

const labels: Record<SaveState, string> = {
  idle: '已就绪',
  pending: '未保存',
  saved: '已保存',
  error: '保存失败',
  conflict: '磁盘已变更',
}

const snapshotLabels: Record<SnapshotState, string> = {
  warming: '缓存预热中',
  ready: '搜索缓存',
  degraded: '缓存已降级',
}

export function StatusBar({
  path = null,
  content = '',
  dirty = false,
  state = 'idle',
  snapshot = null,
}: StatusBarProps) {
  const visualState: SaveState =
    state === 'conflict' || state === 'error' ? state : dirty ? 'pending' : state

  return (
    <footer className="status-bar">
      {path && (
        <>
          <span className={`save-state state-${visualState}`}>
            <span className="status-dot" aria-hidden="true" />
            {labels[visualState]}
          </span>
          <span className="status-separator" aria-hidden="true" />
          <span>{countWords(content)} 字</span>
          <span>{countLines(content)} 行</span>
        </>
      )}

      {snapshot && (
        <span
          className={`snapshot-state snapshot-${snapshot.state}`}
          title={
            snapshot.degradedReason ??
            `${snapshot.cached}/${snapshot.total} 篇已缓存，预算 ${formatBytes(snapshot.budgetBytes)}`
          }
        >
          <span className="status-dot" aria-hidden="true" />
          {snapshotLabels[snapshot.state]} · {formatBytes(snapshot.bytes)}
          <span className="snapshot-count">
            {snapshot.cached}/{snapshot.total} 篇
          </span>
        </span>
      )}

      {path && (
        <span className="status-path" title={path}>
          {path}
        </span>
      )}
    </footer>
  )
}
