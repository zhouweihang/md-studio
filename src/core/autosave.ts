export type SaveHandler = (value: string) => Promise<void>

/**
 * Coalesces rapid edits into one save. `flush()` is the only way to guarantee
 * that the latest text reached the storage layer before switching notes.
 */
export class DebouncedSaver {
  private timer: ReturnType<typeof setTimeout> | null = null
  private pending: string | null = null
  private chain: Promise<void> = Promise.resolve()
  private disposed = false

  constructor(
    private readonly delayMs: number,
    private readonly save: SaveHandler,
  ) {}

  schedule(value: string): void {
    if (this.disposed) return
    this.pending = value

    if (this.timer !== null) clearTimeout(this.timer)
    this.timer = setTimeout(() => {
      this.timer = null
      void this.flush().catch(() => {
        // Errors are surfaced by the caller's save handler; keep the timer
        // callback from producing an unhandled rejection.
      })
    }, this.delayMs)
  }

  flush(): Promise<void> {
    if (this.timer !== null) {
      clearTimeout(this.timer)
      this.timer = null
    }

    if (this.pending === null) return this.chain

    const value = this.pending
    this.pending = null

    const task = this.chain.then(async () => {
      try {
        await this.save(value)
      } catch (error) {
        // Nothing newer is waiting, so keep the failed text for the next retry.
        if (this.pending === null) this.pending = value
        throw error
      }
    })

    this.chain = task.catch(() => {})
    return task
  }

  cancel(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer)
      this.timer = null
    }
    this.pending = null
  }

  dispose(): void {
    this.cancel()
    this.disposed = true
  }

  get hasPending(): boolean {
    return this.pending !== null
  }
}
