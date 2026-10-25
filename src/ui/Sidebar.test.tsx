// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { NoteEntry } from '../core/notes'
import { Sidebar, ROW_HEIGHT } from './Sidebar'

function note(path: string): NoteEntry {
  const name = path.split('/').at(-1) ?? path
  return { path, name, title: name.replace(/\.[^.]+$/, ''), modifiedMs: null }
}

function flatNotes(count: number): NoteEntry[] {
  return Array.from({ length: count }, (_, index) =>
    note(`note-${String(index).padStart(4, '0')}.md`),
  )
}

function renderSidebar(notes: NoteEntry[], overrides: Partial<Parameters<typeof Sidebar>[0]> = {}) {
  return render(
    <Sidebar
      root="C:\\notes"
      notes={notes}
      activePath={null}
      query=""
      loading={false}
      contentHits={[]}
      searching={false}
      summary={null}
      onQueryChange={() => {}}
      onOpenWorkspace={() => {}}
      onRefresh={() => {}}
      onSelect={() => {}}
      onOpenHit={() => {}}
      onCreate={() => {}}
      viewportHeight={240}
      {...overrides}
    />,
  )
}

afterEach(cleanup)

describe('Sidebar tree', () => {
  it('paints only the visible window plus overscan', () => {
    renderSidebar(flatNotes(200))

    const rows = screen.getAllByRole('treeitem')
    expect(rows.length).toBeLessThan(80)
    expect(screen.queryByText('note-0199.md')).toBeNull()
    expect(screen.getByText('note-0000.md')).toBeTruthy()
  })

  it('moves the window when the list scrolls', () => {
    const { container } = renderSidebar(flatNotes(200))
    const scroller = container.querySelector('.tree-scroll') as HTMLDivElement

    scroller.scrollTop = 100 * ROW_HEIGHT
    fireEvent.scroll(scroller)

    expect(screen.getByText('note-0100.md')).toBeTruthy()
    expect(screen.queryByText('note-0000.md')).toBeNull()
    expect(screen.queryByText('note-0199.md')).toBeNull()
  })

  it('collapses folders by default and expands on click', async () => {
    const user = userEvent.setup()
    renderSidebar([note('docs/guide.md'), note('root.md')])

    expect(screen.queryByText('guide.md')).toBeNull()
    const folder = screen.getByRole('treeitem', { name: 'docs' })
    expect(folder.getAttribute('aria-expanded')).toBe('false')

    await user.click(screen.getByRole('button', { name: 'docs' }))

    expect(screen.getByText('guide.md')).toBeTruthy()
    expect(screen.getByRole('treeitem', { name: 'docs' }).getAttribute('aria-expanded')).toBe(
      'true',
    )
  })

  it('reveals folders that contain search matches', async () => {
    const user = userEvent.setup()
    const { rerender } = renderSidebar([note('docs/guide.md'), note('other.md')])

    rerender(
      <Sidebar
        root="C:\\notes"
        notes={[note('docs/guide.md'), note('other.md')]}
        activePath={null}
        query="guide"
        loading={false}
        contentHits={[]}
        searching={false}
        summary={null}
        onQueryChange={() => {}}
        onOpenWorkspace={() => {}}
        onRefresh={() => {}}
        onSelect={() => {}}
        onOpenHit={() => {}}
        onCreate={() => {}}
        viewportHeight={240}
      />,
    )

    expect(screen.getByText('guide.md')).toBeTruthy()
    expect(screen.queryByText('other.md')).toBeNull()
    await user.click(screen.getByText('guide.md'))
  })

  it('walks with arrow keys and opens a note with Enter', async () => {
    const user = userEvent.setup()
    const onSelect = vi.fn()
    renderSidebar([note('a.md'), note('b.md')], { onSelect })

    await user.click(screen.getByRole('button', { name: 'a.md' }))
    expect(onSelect).toHaveBeenCalledWith(note('a.md'))

    await user.keyboard('{ArrowDown}')
    expect(document.activeElement?.textContent).toContain('b.md')

    await user.keyboard('{Enter}')
    expect(onSelect).toHaveBeenCalledWith(note('b.md'))
  })

  it('expands and collapses a focused folder with arrow keys', async () => {
    const user = userEvent.setup()
    renderSidebar([note('docs/guide.md'), note('root.md')])

    await user.click(screen.getByRole('button', { name: 'docs' }))
    expect(screen.getByText('guide.md')).toBeTruthy()

    await user.click(screen.getByRole('button', { name: 'docs' }))
    expect(screen.queryByText('guide.md')).toBeNull()

    await user.click(screen.getByRole('button', { name: 'docs' }))
    await user.keyboard('{ArrowRight}')
    expect(document.activeElement?.textContent).toContain('guide.md')

    await user.keyboard('{ArrowLeft}')
    expect(document.activeElement?.textContent).not.toContain('guide.md')
    expect(screen.getByRole('button', { name: 'docs' })).toBeTruthy()
  })
})
