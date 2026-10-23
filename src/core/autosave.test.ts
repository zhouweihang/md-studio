import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DebouncedSaver } from './autosave'

describe('DebouncedSaver', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('coalesces rapid edits into a single save', async () => {
    const save = vi.fn(async () => {})
    const saver = new DebouncedSaver(700, save)

    saver.schedule('a')
    saver.schedule('ab')
    saver.schedule('abc')

    await vi.advanceTimersByTimeAsync(699)
    expect(save).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)
    expect(save).toHaveBeenCalledTimes(1)
    expect(save).toHaveBeenCalledWith('abc')
  })

  it('flushes pending text immediately and is idempotent', async () => {
    const save = vi.fn(async () => {})
    const saver = new DebouncedSaver(700, save)

    saver.schedule('draft')
    await saver.flush()
    await saver.flush()

    expect(save).toHaveBeenCalledTimes(1)
    expect(save).toHaveBeenCalledWith('draft')
    expect(saver.hasPending).toBe(false)
  })

  it('keeps failed text pending and retries on the next flush', async () => {
    const save = vi.fn(async () => {})
    save.mockRejectedValueOnce(new Error('disk full'))

    const saver = new DebouncedSaver(700, save)
    saver.schedule('important')

    await expect(saver.flush()).rejects.toThrow('disk full')
    expect(saver.hasPending).toBe(true)

    await saver.flush()
    expect(save).toHaveBeenCalledTimes(2)
    expect(save).toHaveBeenLastCalledWith('important')
    expect(saver.hasPending).toBe(false)
  })

  it('cancel drops pending text without saving', async () => {
    const save = vi.fn(async () => {})
    const saver = new DebouncedSaver(700, save)

    saver.schedule('discard')
    saver.cancel()

    await vi.advanceTimersByTimeAsync(1000)
    expect(save).not.toHaveBeenCalled()
    expect(saver.hasPending).toBe(false)
  })
})
