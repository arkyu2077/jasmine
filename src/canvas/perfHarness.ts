import "../styles/app.css";
import { CanvasScene, type SceneStats } from "./scene";
import type { Asset, Placement, PlacementUpdate, Shape } from "../types";

interface ScenarioSample {
  name: string;
  before: SceneStats;
  after: SceneStats;
  maxFrameMs: number;
  minFps: number;
  maxInputCoalesced: number;
  maxInputDelayMs: number;
  maxSnapCandidates: number;
  maxHitCandidates: number;
  maxMarqueeCandidates: number;
  elapsedMs: number;
}

interface PerfResult {
  ok: boolean;
  count: number;
  renderer: string;
  thresholds: Record<string, number>;
  initial: SceneStats;
  final: SceneStats;
  scenarios: ScenarioSample[];
  failures: string[];
}

interface PerfApi {
  ready: boolean;
  stats: SceneStats | null;
  result: PerfResult | null;
  run: (count?: number) => Promise<PerfResult>;
}

declare global {
  interface Window {
    __JASMINE_CANVAS_PERF__?: PerfApi;
  }
}

const DEFAULT_COUNT = 10_000;
const FRAME_WAIT_MS = 520;
const thresholds = {
  maxActivePlacements: 260,
  maxFrameMs: 50,
  minFps: 20,
  maxSnapCandidates: 80,
  maxHitCandidates: 8,
  maxMarqueeCandidates: 180,
  minSnapCandidates: 1,
  minHitCandidates: 1,
  minMarqueeCandidates: 1,
  minInputCoalesced: 1,
};

const hostNode = document.getElementById("perf-host");
const statusNode = document.getElementById("perf-status");
const runButtonNode = document.getElementById("perf-run");
const resultNode = document.getElementById("perf-result");

if (!hostNode || !statusNode || !(runButtonNode instanceof HTMLButtonElement) || !resultNode) {
  throw new Error("missing perf harness DOM");
}

const host = hostNode;
const statusEl = statusNode;
const runBtn = runButtonNode;
const resultEl = resultNode;

let scene: CanvasScene | null = null;
let latestStats: SceneStats | null = null;
let statsHistory: SceneStats[] = [];
let placements = new Map<string, Placement>();
let assets = new Map<string, Asset>();
let annotations = new Map<string, Shape[]>();

function setStatus(text: string): void {
  statusEl.textContent = text;
}

function makeAsset(): Asset {
  return {
    id: "synthetic-asset",
    path: "synthetic.png",
    width: 320,
    height: 220,
    mime: "image/png",
    createdAt: 0,
    origin: "imported",
  };
}

function makePlacements(count: number): Map<string, Placement> {
  const out = new Map<string, Placement>();
  const cols = Math.ceil(Math.sqrt(count));
  for (let i = 0; i < count; i++) {
    let x = 0;
    let y = 0;
    if (i === 1) {
      x = 520;
    } else if (i > 1) {
      const j = i - 2;
      x = ((j % cols) - cols / 2) * 720;
      y = (Math.floor(j / cols) + 1) * 620;
    }
    out.set(`p${i}`, {
      id: `p${i}`,
      assetId: "synthetic-asset",
      x,
      y,
      scale: 1,
      rotation: 0,
      z: i,
    });
  }
  return out;
}

function applyUpdates(updates: PlacementUpdate[]): void {
  if (!scene) return;
  const next = new Map(placements);
  for (const update of updates) {
    const p = next.get(update.id);
    if (!p) continue;
    next.set(update.id, {
      ...p,
      x: update.x,
      y: update.y,
      scale: update.scale,
      rotation: update.rotation,
      z: update.z,
    });
  }
  placements = next;
  scene.setData(null, placements, assets, annotations, new Map());
}

async function nextFrame(): Promise<void> {
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
}

async function wait(ms: number): Promise<void> {
  await new Promise<void>((resolve) => window.setTimeout(resolve, ms));
}

async function waitForStats(): Promise<SceneStats> {
  const start = performance.now();
  while (!latestStats) {
    if (performance.now() - start > 3_000) throw new Error("timed out waiting for scene stats");
    await wait(50);
  }
  return latestStats;
}

function currentStats(): SceneStats {
  if (!latestStats) throw new Error("stats unavailable");
  return latestStats;
}

function scenarioStats(name: string, before: SceneStats, startIndex: number, elapsedMs: number): ScenarioSample {
  const slice = statsHistory.slice(startIndex);
  const after = currentStats();
  return {
    name,
    before,
    after,
    maxFrameMs: Math.max(...slice.map((s) => s.frameMs), after.frameMs),
    minFps: Math.min(...slice.map((s) => s.fps), after.fps),
    maxInputCoalesced: Math.max(...slice.map((s) => s.inputCoalesced), after.inputCoalesced),
    maxInputDelayMs: Math.max(...slice.map((s) => s.inputMaxDelayMs), after.inputMaxDelayMs),
    maxSnapCandidates: Math.max(...slice.map((s) => s.snapCandidates), after.snapCandidates),
    maxHitCandidates: Math.max(...slice.map((s) => s.hitCandidates), after.hitCandidates),
    maxMarqueeCandidates: Math.max(...slice.map((s) => s.marqueeCandidates), after.marqueeCandidates),
    elapsedMs,
  };
}

function canvasEl(): HTMLCanvasElement {
  const canvas = host.querySelector("canvas");
  if (!(canvas instanceof HTMLCanvasElement)) throw new Error("canvas not found");
  return canvas;
}

function dispatchWheel(deltaX: number, deltaY: number, opts: Partial<WheelEventInit> = {}): void {
  const canvas = canvasEl();
  const rect = canvas.getBoundingClientRect();
  canvas.dispatchEvent(
    new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      clientX: rect.left + rect.width / 2,
      clientY: rect.top + rect.height / 2,
      deltaX,
      deltaY,
      ...opts,
    })
  );
}

function dispatchPointer(type: string, x: number, y: number): void {
  const canvas = canvasEl();
  canvas.dispatchEvent(
    new PointerEvent(type, {
      bubbles: true,
      cancelable: true,
      pointerId: 1,
      pointerType: "mouse",
      isPrimary: true,
      button: type === "pointerup" ? 0 : 0,
      buttons: type === "pointerup" ? 0 : 1,
      clientX: x,
      clientY: y,
    })
  );
}

function dispatchContextMenu(x: number, y: number): void {
  canvasEl().dispatchEvent(
    new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      button: 2,
      buttons: 2,
      clientX: x,
      clientY: y,
    })
  );
}

async function focusPrimaryPlacement(): Promise<void> {
  if (!scene) throw new Error("scene unavailable");
  scene.setSelection([]);
  scene.focusPlacement("p0");
  await wait(FRAME_WAIT_MS);
}

async function runScenario(name: string, action: () => Promise<void>): Promise<ScenarioSample> {
  await waitForStats();
  const before = currentStats();
  const startIndex = statsHistory.length;
  const start = performance.now();
  setStatus(`running ${name}`);
  await action();
  await wait(FRAME_WAIT_MS);
  const elapsedMs = performance.now() - start;
  return scenarioStats(name, before, startIndex, elapsedMs);
}

async function runPerf(count = DEFAULT_COUNT): Promise<PerfResult> {
  if (!scene) throw new Error("scene unavailable");
  runBtn.disabled = true;
  try {
    setStatus(`building ${count}`);
    resultEl.textContent = "";
    latestStats = null;
    statsHistory = [];
    assets = new Map([["synthetic-asset", makeAsset()]]);
    placements = makePlacements(count);
    annotations = new Map();
    scene.setData(null, placements, assets, annotations, new Map());
    scene.setSelection([]);
    scene.focusPlacement("p0");
    await waitForStats();

    const scenarios: ScenarioSample[] = [];
    scenarios.push(
      await runScenario("wheel pan", async () => {
        await focusPrimaryPlacement();
        for (let i = 0; i < 180; i++) dispatchWheel(18, 9);
        await nextFrame();
      })
    );
    scenarios.push(
      await runScenario("wheel zoom", async () => {
        await focusPrimaryPlacement();
        for (let i = 0; i < 120; i++) dispatchWheel(0, i % 2 === 0 ? -24 : 18, { ctrlKey: true });
        await nextFrame();
      })
    );
    scenarios.push(
      await runScenario("hit test", async () => {
        await focusPrimaryPlacement();
        const rect = canvasEl().getBoundingClientRect();
        for (let i = 0; i < 20; i++) dispatchContextMenu(rect.left + rect.width / 2, rect.top + rect.height / 2);
        await nextFrame();
      })
    );
    scenarios.push(
      await runScenario("drag with snapping", async () => {
        await focusPrimaryPlacement();
        const rect = canvasEl().getBoundingClientRect();
        const sx = rect.left + rect.width / 2;
        const sy = rect.top + rect.height / 2;
        dispatchPointer("pointerdown", sx, sy);
        for (let i = 1; i <= 80; i++) dispatchPointer("pointermove", sx + i * 4.5, sy + Math.sin(i / 4) * 8);
        dispatchPointer("pointerup", sx + 360, sy);
        await nextFrame();
      })
    );
    scenarios.push(
      await runScenario("marquee selection", async () => {
        await focusPrimaryPlacement();
        const rect = canvasEl().getBoundingClientRect();
        const sx = rect.left + rect.width * 0.18;
        const sy = rect.top + rect.height * 0.66;
        const ex = rect.left + rect.width * 0.62;
        const ey = rect.top + rect.height * 0.88;
        dispatchPointer("pointerdown", sx, sy);
        for (let i = 1; i <= 48; i++) {
          dispatchPointer("pointermove", sx + ((ex - sx) * i) / 48, sy + ((ey - sy) * i) / 48);
        }
        dispatchPointer("pointerup", ex, ey);
        await nextFrame();
      })
    );

    const initial = scenarios[0]?.before ?? currentStats();
    const final = currentStats();
    const failures: string[] = [];
    if (initial.placements !== count) failures.push(`expected ${count} placements, got ${initial.placements}`);
    if (initial.activePlacements > thresholds.maxActivePlacements) {
      failures.push(`active placements ${initial.activePlacements} > ${thresholds.maxActivePlacements}`);
    }
    for (const s of scenarios) {
      if (s.maxFrameMs > thresholds.maxFrameMs) failures.push(`${s.name} frame ${s.maxFrameMs}ms > ${thresholds.maxFrameMs}ms`);
      if (s.minFps < thresholds.minFps) failures.push(`${s.name} fps ${s.minFps} < ${thresholds.minFps}`);
    }
    const drag = scenarios.find((s) => s.name === "drag with snapping");
    const hit = scenarios.find((s) => s.name === "hit test");
    const marquee = scenarios.find((s) => s.name === "marquee selection");
    const pan = scenarios.find((s) => s.name === "wheel pan");
    const zoom = scenarios.find((s) => s.name === "wheel zoom");
    if (drag && drag.maxSnapCandidates > thresholds.maxSnapCandidates) {
      failures.push(`snap candidates ${drag.maxSnapCandidates} > ${thresholds.maxSnapCandidates}`);
    }
    if (drag && drag.maxSnapCandidates < thresholds.minSnapCandidates) {
      failures.push(`snap candidates ${drag.maxSnapCandidates} < ${thresholds.minSnapCandidates}`);
    }
    if (hit && hit.maxHitCandidates > thresholds.maxHitCandidates) {
      failures.push(`hit candidates ${hit.maxHitCandidates} > ${thresholds.maxHitCandidates}`);
    }
    if (hit && hit.maxHitCandidates < thresholds.minHitCandidates) {
      failures.push(`hit candidates ${hit.maxHitCandidates} < ${thresholds.minHitCandidates}`);
    }
    if (marquee && marquee.maxMarqueeCandidates > thresholds.maxMarqueeCandidates) {
      failures.push(`marquee candidates ${marquee.maxMarqueeCandidates} > ${thresholds.maxMarqueeCandidates}`);
    }
    if (marquee && marquee.maxMarqueeCandidates < thresholds.minMarqueeCandidates) {
      failures.push(`marquee candidates ${marquee.maxMarqueeCandidates} < ${thresholds.minMarqueeCandidates}`);
    }
    if (pan && pan.maxInputCoalesced < thresholds.minInputCoalesced) failures.push("wheel pan did not coalesce input");
    if (zoom && zoom.maxInputCoalesced < thresholds.minInputCoalesced) failures.push("wheel zoom did not coalesce input");

    const result: PerfResult = {
      ok: failures.length === 0,
      count,
      renderer: final.renderer,
      thresholds,
      initial,
      final,
      scenarios,
      failures,
    };
    window.__JASMINE_CANVAS_PERF__!.result = result;
    resultEl.textContent = JSON.stringify(result, null, 2);
    setStatus(result.ok ? "done: pass" : `done: ${failures.length} failures`);
    return result;
  } finally {
    runBtn.disabled = false;
  }
}

async function boot(): Promise<void> {
  scene = new CanvasScene();
  await scene.init(host, {
    onStats: (stats) => {
      latestStats = stats;
      statsHistory.push(stats);
      window.__JASMINE_CANVAS_PERF__!.stats = stats;
      if (!window.__JASMINE_CANVAS_PERF__!.result) {
        setStatus(`${stats.activePlacements}/${stats.placements} active · ${stats.frameMs}ms`);
      }
    },
    onSelectionChange: () => undefined,
    onCommitMoves: (updates) => applyUpdates(updates),
    onAnnotate: (placementId, shapes) => {
      annotations = new Map(annotations);
      annotations.set(placementId, shapes);
      scene?.setData(null, placements, assets, annotations, new Map());
    },
  });
  window.__JASMINE_CANVAS_PERF__ = { ready: true, stats: latestStats, result: null, run: runPerf };
  runBtn.addEventListener("click", () => {
    void runPerf();
  });
  setStatus("ready");
}

window.__JASMINE_CANVAS_PERF__ = { ready: false, stats: null, result: null, run: runPerf };
void boot();
