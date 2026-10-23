// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { renderMarkdown } from './render'

describe('renderMarkdown', () => {
  it('renders headings, lists and GFM tables', () => {
    const html = renderMarkdown('# Title\n\n- one\n- two\n\n| A | B |\n| - | - |\n| 1 | 2 |')
    expect(html).toContain('<h1>Title</h1>')
    expect(html).toContain('<li>one</li>')
    expect(html).toContain('<table>')
    expect(html).toContain('<td>1</td>')
  })

  it('removes scripts and event handler attributes', () => {
    const html = renderMarkdown('<script>alert(1)</script>\n\n<img src="x" onerror="alert(1)">')
    expect(html).not.toContain('<script')
    expect(html).not.toContain('onerror')
    expect(html).not.toContain('alert(1)')
  })

  it('renders inline code and fenced code blocks', () => {
    const html = renderMarkdown('Use `bun test`.\n\n```ts\nconst x = 1\n```')
    expect(html).toContain('<code>bun test</code>')
    expect(html).toContain('<pre><code class="language-ts">')
  })
})
