import { describe, expect, it } from "vitest";
import { SpatialIndex, boundsFromEdges, expandBounds } from "./spatialIndex";

describe("SpatialIndex", () => {
  it("returns only entries whose bounds intersect the query rect", () => {
    const index = new SpatialIndex(100);
    index.set("a", boundsFromEdges(0, 0, 50, 50));
    index.set("b", boundsFromEdges(220, 0, 260, 40));
    index.set("c", boundsFromEdges(-200, -200, -150, -150));

    expect(index.queryRect(boundsFromEdges(-10, -10, 80, 80)).sort()).toEqual(["a"]);
    expect(index.queryRect(boundsFromEdges(200, -10, 250, 20)).sort()).toEqual(["b"]);
  });

  it("supports point queries across cells", () => {
    const index = new SpatialIndex(64);
    index.set("a", boundsFromEdges(60, 60, 130, 130));
    index.set("b", boundsFromEdges(131, 131, 180, 180));

    expect(index.queryPoint(100, 100)).toEqual(["a"]);
    expect(index.queryPoint(150, 150)).toEqual(["b"]);
    expect(index.queryPoint(300, 300)).toEqual([]);
  });

  it("updates and removes entries without leaving stale cell references", () => {
    const index = new SpatialIndex(100);
    index.set("a", boundsFromEdges(0, 0, 50, 50));
    index.set("a", boundsFromEdges(500, 500, 520, 520));

    expect(index.queryRect(boundsFromEdges(0, 0, 100, 100))).toEqual([]);
    expect(index.queryPoint(510, 510)).toEqual(["a"]);

    index.remove("a");
    expect(index.queryPoint(510, 510)).toEqual([]);
    expect(index.stats().entries).toBe(0);
  });

  it("keeps very large entries queryable without indexing every covered cell", () => {
    const index = new SpatialIndex(10);
    index.set("large", boundsFromEdges(-10_000, -10_000, 10_000, 10_000));
    index.set("small", boundsFromEdges(20, 20, 30, 30));

    expect(index.stats().largeEntries).toBe(1);
    expect(index.queryPoint(25, 25).sort()).toEqual(["large", "small"]);
    expect(index.queryRect(expandBounds(boundsFromEdges(500, 500, 510, 510), 1))).toEqual(["large"]);
  });

  it("computes a global union for fit-all and minimap operations", () => {
    const index = new SpatialIndex(100);
    index.set("a", boundsFromEdges(0, 0, 10, 20));
    index.set("b", boundsFromEdges(-30, 40, -10, 80));

    expect(index.allBounds()).toEqual(boundsFromEdges(-30, 0, 10, 80));
  });
});
