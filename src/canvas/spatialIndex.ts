export interface SpatialBounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  centerX: number;
  centerY: number;
}

interface IndexedEntry {
  bounds: SpatialBounds;
  cells: string[] | null;
}

export interface SpatialIndexStats {
  entries: number;
  cells: number;
  largeEntries: number;
  cellSize: number;
}

const MAX_CELLS_PER_ENTRY = 256;
const MAX_QUERY_CELLS = 4096;

export function boundsFromEdges(minX: number, minY: number, maxX: number, maxY: number): SpatialBounds {
  return {
    minX,
    minY,
    maxX,
    maxY,
    centerX: (minX + maxX) / 2,
    centerY: (minY + maxY) / 2,
  };
}

export function boundsIntersect(a: SpatialBounds, b: SpatialBounds): boolean {
  return a.maxX >= b.minX && a.minX <= b.maxX && a.maxY >= b.minY && a.minY <= b.maxY;
}

export function boundsContainPoint(b: SpatialBounds, x: number, y: number): boolean {
  return x >= b.minX && x <= b.maxX && y >= b.minY && y <= b.maxY;
}

export function expandBounds(b: SpatialBounds, pad: number): SpatialBounds {
  return boundsFromEdges(b.minX - pad, b.minY - pad, b.maxX + pad, b.maxY + pad);
}

export class SpatialIndex {
  private readonly entries = new Map<string, IndexedEntry>();
  private readonly cells = new Map<string, Set<string>>();
  private readonly largeIds = new Set<string>();

  constructor(private readonly cellSize = 1024) {}

  clear(): void {
    this.entries.clear();
    this.cells.clear();
    this.largeIds.clear();
  }

  set(id: string, bounds: SpatialBounds): void {
    this.remove(id);
    if (!this.isFiniteBounds(bounds)) return;
    const cells = this.cellsForBounds(bounds);
    if (!cells) {
      this.largeIds.add(id);
      this.entries.set(id, { bounds, cells: null });
      return;
    }
    for (const key of cells) {
      let bucket = this.cells.get(key);
      if (!bucket) {
        bucket = new Set<string>();
        this.cells.set(key, bucket);
      }
      bucket.add(id);
    }
    this.entries.set(id, { bounds, cells });
  }

  remove(id: string): void {
    const entry = this.entries.get(id);
    if (!entry) return;
    if (entry.cells) {
      for (const key of entry.cells) {
        const bucket = this.cells.get(key);
        if (!bucket) continue;
        bucket.delete(id);
        if (bucket.size === 0) this.cells.delete(key);
      }
    } else {
      this.largeIds.delete(id);
    }
    this.entries.delete(id);
  }

  get(id: string): SpatialBounds | null {
    return this.entries.get(id)?.bounds ?? null;
  }

  ids(): IterableIterator<string> {
    return this.entries.keys();
  }

  allBounds(): SpatialBounds | null {
    let union: SpatialBounds | null = null;
    for (const entry of this.entries.values()) {
      union = union ? boundsFromEdges(
        Math.min(union.minX, entry.bounds.minX),
        Math.min(union.minY, entry.bounds.minY),
        Math.max(union.maxX, entry.bounds.maxX),
        Math.max(union.maxY, entry.bounds.maxY)
      ) : entry.bounds;
    }
    return union;
  }

  queryRect(rect: SpatialBounds): string[] {
    if (!this.isFiniteBounds(rect)) return [];
    const result = new Set<string>();
    const cells = this.cellsForBounds(rect, MAX_QUERY_CELLS);
    if (!cells) {
      for (const id of this.entries.keys()) result.add(id);
    } else {
      for (const key of cells) {
        const bucket = this.cells.get(key);
        if (!bucket) continue;
        for (const id of bucket) result.add(id);
      }
      for (const id of this.largeIds) result.add(id);
    }
    return [...result].filter((id) => {
      const entry = this.entries.get(id);
      return !!entry && boundsIntersect(entry.bounds, rect);
    });
  }

  queryPoint(x: number, y: number): string[] {
    if (!Number.isFinite(x) || !Number.isFinite(y)) return [];
    const result = new Set<string>();
    const bucket = this.cells.get(this.cellKey(this.cellCoord(x), this.cellCoord(y)));
    if (bucket) for (const id of bucket) result.add(id);
    for (const id of this.largeIds) result.add(id);
    return [...result].filter((id) => {
      const entry = this.entries.get(id);
      return !!entry && boundsContainPoint(entry.bounds, x, y);
    });
  }

  stats(): SpatialIndexStats {
    return {
      entries: this.entries.size,
      cells: this.cells.size,
      largeEntries: this.largeIds.size,
      cellSize: this.cellSize,
    };
  }

  private isFiniteBounds(b: SpatialBounds): boolean {
    return (
      Number.isFinite(b.minX) &&
      Number.isFinite(b.minY) &&
      Number.isFinite(b.maxX) &&
      Number.isFinite(b.maxY) &&
      b.maxX >= b.minX &&
      b.maxY >= b.minY
    );
  }

  private cellCoord(v: number): number {
    return Math.floor(v / this.cellSize);
  }

  private cellKey(x: number, y: number): string {
    return `${x}:${y}`;
  }

  private cellsForBounds(bounds: SpatialBounds, maxCells = MAX_CELLS_PER_ENTRY): string[] | null {
    const x0 = this.cellCoord(bounds.minX);
    const y0 = this.cellCoord(bounds.minY);
    const x1 = this.cellCoord(bounds.maxX);
    const y1 = this.cellCoord(bounds.maxY);
    const count = (x1 - x0 + 1) * (y1 - y0 + 1);
    if (count > maxCells) return null;
    const keys: string[] = [];
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) keys.push(this.cellKey(x, y));
    }
    return keys;
  }
}
