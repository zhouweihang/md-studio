// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import type { SnapshotStatus } from '../core/search'
import { StatusBar } from './StatusBar'

function status(state: SnapshotStatus['state'], overrides: Partial<SnapshotStatus> = {}) {
  return {
    state,
    total: 1_000,
    cached: 900,
    bytes: 1.5 * 1024 * 1024,
    budgetBytes: 256 * 1024 * 1024,
    degradedReason: null,
    ...overrides,
  } satisfies SnapshotStatus
}

afterEach(cleanup)

describe('StatusBar snapshot chip', () => {
  it('shows the warming state with cached bytes and note counts', () => {
    render(<StatusBar snapshot={status('warming')} />)

    expect(screen.getByText(/缓存预热中 · 1.5 MB/)).toBeTruthy()
    expect(screen.getByText('900/1000 篇')).toBeTruthy()
  })

  it('shows the ready state', () => {
    render(<StatusBar snapshot={status('ready')} />)

    expect(screen.getByText(/搜索缓存 · 1.5 MB/)).toBeTruthy()
  })

  it('shows the degraded state and explains why in the tooltip', () => {
    render(
      <StatusBar
        snapshot={status('degraded', { degradedReason: 'cannot watch workspace' })}
      />,
    )

    const chip = screen.getByTitle('cannot watch workspace')
    expect(chip.textContent).toContain('缓存已降级')
  })

  it('keeps note statistics out of the bar when no note is open', () => {
    const { container } = render(<StatusBar snapshot={status('ready')} />)

    expect(container.querySelector('.save-state')).toBeNull()
    expect(container.querySelector('.status-path')).toBeNull()
  })

  it('renders only note statistics when there is no snapshot', () => {
    const { container } = render(<StatusBar path="hello.md" content="one two" />)

    expect(container.querySelector('.snapshot-state')).toBeNull()
    expect(container.querySelector('.status-path')?.textContent).toBe('hello.md')
  })
})
