export interface RafInputStats {
  frames: number;
  flushes: number;
  coalesced: number;
  maxDelayMs: number;
}

const emptyStats = (): RafInputStats => ({ frames: 0, flushes: 0, coalesced: 0, maxDelayMs: 0 });

export class RafInputScheduler<T> {
  private raf = 0;
  private pending: T | null = null;
  private scheduledAt = 0;
  private stats = emptyStats();

  constructor(
    private readonly consume: (value: T) => void,
    private readonly merge: (prev: T, next: T) => T = (_prev, next) => next
  ) {}

  schedule(value: T): void {
    if (this.pending) {
      this.pending = this.merge(this.pending, value);
      this.stats.coalesced += 1;
    } else {
      this.pending = value;
    }
    if (this.raf) return;
    this.scheduledAt = performance.now();
    this.raf = requestAnimationFrame(() => this.flush());
    this.stats.frames += 1;
  }

  flush(): void {
    if (this.raf) {
      cancelAnimationFrame(this.raf);
      this.raf = 0;
    }
    const value = this.pending;
    this.pending = null;
    if (!value) return;
    this.stats.flushes += 1;
    this.stats.maxDelayMs = Math.max(this.stats.maxDelayMs, performance.now() - this.scheduledAt);
    this.consume(value);
  }

  cancel(): void {
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
    this.pending = null;
  }

  sampleStats(): RafInputStats {
    const out = this.stats;
    this.stats = emptyStats();
    return out;
  }
}
