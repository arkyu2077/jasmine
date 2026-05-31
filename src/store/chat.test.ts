import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The store calls into Tauri via `ipc.*`. Under vitest (node, no Tauri) those
// would throw, so stub every method as a resolved-promise no-op. Importing the
// store itself is side-effect free; only ipc *calls* need neutralising.
vi.mock("../lib/ipc", () => ({
  ipc: new Proxy(
    {},
    { get: () => () => Promise.resolve(undefined) },
  ),
}));

import { useChatStore, type ChatBlock, type ChatMessage } from "./chat";
import { messages as i18n } from "../i18n/messages";
import type { CodexEvent } from "../types";

type AsstMsg = Extract<ChatMessage, { role: "assistant" }>;

/** Seed a turn mid-stream: a user message + a streaming assistant message
 *  carrying `blocks`, with turnStatus "running" (as startTurn would leave it). */
function seedRunningTurn(blocks: ChatBlock[] = []): void {
  const now = Date.now();
  useChatStore.setState({
    turnStatus: "running",
    transportStatus: null,
    turnStartedAt: now,
    turnLastActivityAt: now,
    messages: [
      { id: "u1", role: "user", text: "hi", refs: [] },
      { id: "a1", role: "assistant", blocks, status: "streaming" },
    ],
  });
}

function lastAssistant(): AsstMsg | null {
  const ms = useChatStore.getState().messages;
  for (let i = ms.length - 1; i >= 0; i--) {
    if (ms[i].role === "assistant") return ms[i] as AsstMsg;
  }
  return null;
}

/** THE invariant this whole mechanism guards: after any terminal event the turn
 *  must be idle, every spinner settled, and the message must show *something*
 *  the user can read — never a vanished spinner with no explanation. */
function expectSettledAndExplained(): AsstMsg {
  const st = useChatStore.getState();
  expect(st.turnStatus, "turn must leave the running state").toBe("idle");
  const m = lastAssistant();
  expect(m, "a terminal assistant message must exist").not.toBeNull();
  for (const b of m!.blocks) {
    if (b.type === "thinking") expect(b.active, "no thinking spinner left running").toBe(false);
    if (b.type === "tool") expect(b.status, "no tool spinner left running").toBe("done");
    if (b.type === "image") expect(b.status, "no image left 'generating'").not.toBe("generating");
  }
  const visible = m!.blocks.length > 0 || (m!.status === "error" && Boolean(m!.error));
  expect(visible, "the outcome must be visible (blocks or an error reason)").toBe(true);
  return m!;
}

const text = (t: string): ChatBlock => ({ type: "text", text: t });
const generatingImage = (): ChatBlock => ({ type: "image", placementId: "ph1", status: "generating", startedAt: Date.now() });

beforeEach(() => {
  useChatStore.getState().reset();
});
afterEach(() => {
  useChatStore.getState().reset(); // stops the module-level watchdog timer
});

describe("turn lifecycle invariant — every terminal event settles & explains", () => {
  const terminalEvents: { name: string; event: CodexEvent }[] = [
    { name: "completed", event: { kind: "turnComplete", status: "completed" } },
    { name: "aborted", event: { kind: "turnComplete", status: "aborted" } },
    { name: "interrupted", event: { kind: "turnComplete", status: "interrupted" } },
    { name: "failed-with-reason", event: { kind: "turnComplete", status: "failed", error: "boom" } },
    { name: "failed-no-reason", event: { kind: "turnComplete", status: "failed" } },
    { name: "fatal-error", event: { kind: "error", message: "fatal" } },
  ];

  for (const { name, event } of terminalEvents) {
    it(`with a preamble block: ${name}`, () => {
      seedRunningTurn([text("我先看一下参考图…")]);
      useChatStore.getState().handleEvent(event);
      expectSettledAndExplained();
    });

    it(`with no prior content: ${name}`, () => {
      seedRunningTurn([]);
      useChatStore.getState().handleEvent(event);
      expectSettledAndExplained();
    });
  }
});

describe("specific message-flow guarantees", () => {
  it("completed with zero substance leaves an explanatory note (not a void)", () => {
    seedRunningTurn([]);
    useChatStore.getState().handleEvent({ kind: "turnComplete", status: "completed" });
    const m = expectSettledAndExplained();
    expect(m.status).toBe("done");
    const note = m.blocks.find((b) => b.type === "note");
    expect(note && note.type === "note" && note.text).toBe(i18n.en["chat.turn.empty"]);
  });

  it("completed WITH content does not inject a spurious note", () => {
    seedRunningTurn([text("here is your result")]);
    useChatStore.getState().handleEvent({ kind: "turnComplete", status: "completed" });
    const m = lastAssistant()!;
    expect(m.status).toBe("done");
    expect(m.blocks.some((b) => b.type === "note")).toBe(false);
  });

  it("aborted surfaces a localized 'Stopped.' note even atop a preamble", () => {
    seedRunningTurn([text("working…")]);
    useChatStore.getState().handleEvent({ kind: "turnComplete", status: "aborted" });
    const m = expectSettledAndExplained();
    expect(m.status).toBe("error");
    const note = m.blocks.find((b) => b.type === "note");
    expect(note && note.type === "note" && note.text).toBe(i18n.en["chat.turn.stopped"]);
  });

  it("failed surfaces codex's specific reason as a visible note", () => {
    seedRunningTurn([text("preamble")]);
    useChatStore.getState().handleEvent({ kind: "turnComplete", status: "failed", error: "model provider 502" });
    const m = expectSettledAndExplained();
    const note = m.blocks.find((b) => b.type === "note");
    expect(note && note.type === "note" && note.text).toBe("model provider 502");
    expect(m.error).toBe("model provider 502");
  });

  it("settles an image block stuck in 'generating' when the turn completes", () => {
    seedRunningTurn([generatingImage()]);
    useChatStore.getState().handleEvent({ kind: "turnComplete", status: "completed" });
    const m = expectSettledAndExplained();
    const img = m.blocks.find((b) => b.type === "image");
    expect(img && img.type === "image" && img.status).toBe("failed");
  });

  it("a fatal error always shows its message", () => {
    seedRunningTurn([text("partial output")]);
    useChatStore.getState().handleEvent({ kind: "error", message: "sidecar crashed" });
    const m = expectSettledAndExplained();
    expect(m.status).toBe("error");
    expect(m.blocks.some((b) => b.type === "note" && b.text === "sidecar crashed")).toBe(true);
  });
});
