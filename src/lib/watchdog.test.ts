import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { InactivityWatchdog } from "./watchdog";

// The watchdog is the last-resort guarantee that a wedged turn never leaves the
// user waiting forever. These tests pin that behaviour with fake timers so they
// run instantly and deterministically.

const TIMEOUT = 10_000;
const INTERVAL = 1_000;

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

function makeWatchdog() {
  const onFire = vi.fn();
  const wd = new InactivityWatchdog({ timeoutMs: TIMEOUT, intervalMs: INTERVAL, onFire });
  return { wd, onFire };
}

describe("InactivityWatchdog — never-stuck guarantee", () => {
  it("fires once after timeoutMs of silence", () => {
    const { wd, onFire } = makeWatchdog();
    wd.start();
    vi.advanceTimersByTime(TIMEOUT - INTERVAL);
    expect(onFire).not.toHaveBeenCalled();
    vi.advanceTimersByTime(INTERVAL);
    expect(onFire).toHaveBeenCalledTimes(1);
  });

  it("auto-stops after firing (does not fire repeatedly)", () => {
    const { wd, onFire } = makeWatchdog();
    wd.start();
    vi.advanceTimersByTime(TIMEOUT * 3);
    expect(onFire).toHaveBeenCalledTimes(1);
    expect(wd.running).toBe(false);
  });

  it("touch() resets the inactivity clock so a healthy turn never trips", () => {
    const { wd, onFire } = makeWatchdog();
    wd.start();
    // Keep "making progress" every half-timeout — must never fire.
    for (let i = 0; i < 5; i++) {
      vi.advanceTimersByTime(TIMEOUT / 2);
      wd.touch();
    }
    expect(onFire).not.toHaveBeenCalled();
    // Then go silent → fires.
    vi.advanceTimersByTime(TIMEOUT);
    expect(onFire).toHaveBeenCalledTimes(1);
  });

  it("stop() disarms it entirely", () => {
    const { wd, onFire } = makeWatchdog();
    wd.start();
    wd.stop();
    vi.advanceTimersByTime(TIMEOUT * 2);
    expect(onFire).not.toHaveBeenCalled();
  });

  it("touch() after stop() does not silently re-arm", () => {
    const { wd, onFire } = makeWatchdog();
    wd.start();
    wd.stop();
    wd.touch();
    vi.advanceTimersByTime(TIMEOUT * 2);
    expect(onFire).not.toHaveBeenCalled();
  });

  it("pause() halts the clock; resume() re-baselines without an immediate fire", () => {
    const { wd, onFire } = makeWatchdog();
    wd.start();
    wd.pause();
    vi.advanceTimersByTime(TIMEOUT * 3); // long user think-time
    expect(onFire).not.toHaveBeenCalled();
    wd.resume();
    vi.advanceTimersByTime(TIMEOUT - INTERVAL);
    expect(onFire).not.toHaveBeenCalled(); // clock restarted at resume
    vi.advanceTimersByTime(INTERVAL);
    expect(onFire).toHaveBeenCalledTimes(1);
  });

  it("OS suspension (a huge tick gap) re-baselines instead of false-firing", () => {
    const { wd, onFire } = makeWatchdog();
    wd.start();
    // Simulate App Nap / lid-close: wall clock jumps far beyond the timeout
    // between two ticks. The watchdog must treat it as off-clock, not idle.
    vi.setSystemTime(Date.now() + TIMEOUT * 10);
    vi.advanceTimersByTime(INTERVAL); // one tick observes the giant gap
    expect(onFire).not.toHaveBeenCalled();
    // After re-baselining, a normal silent window still fires.
    vi.advanceTimersByTime(TIMEOUT);
    expect(onFire).toHaveBeenCalledTimes(1);
  });
});
