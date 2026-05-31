// Full raw-process logger for the relay (中转站) reasoning question.
//
// Native Codex talks the Responses API → reasoning arrives as structured items
// → the UI shows "已思考 Ns". The relay path goes through Jasmine's Chat
// Completions adapter (provider_adapter.rs). This script bypasses the app and
// hits the relay directly, dumping the ENTIRE raw exchange for BOTH request
// modes — streaming (SSE) and non-streaming ("sync" JSON) — so we can see
// exactly what the relay returns and where reasoning lives (if anywhere).
//
// USAGE:
//   node scripts/diagnose-reasoning.mjs                 # uses ~/.jasmine/config.json
//   node scripts/diagnose-reasoning.mjs --no-effort     # omit reasoning_effort
//   RELAY_API_KEY=sk-... RELAY_BASE_URL=https://api.x/v1 RELAY_MODEL=foo node scripts/diagnose-reasoning.mjs
//
// Sends a tiny reasoning-eliciting prompt to YOUR relay (uses a little quota).

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const MAX_DUMP = 24_000; // cap each raw body dump so the console stays readable

function loadConfig() {
  const env = {
    base_url: process.env.RELAY_BASE_URL,
    api_key: process.env.RELAY_API_KEY,
    model: process.env.RELAY_MODEL,
  };
  if (env.base_url && env.api_key && env.model) return { ...env, name: "relay(env)", source: "env" };

  const path = join(homedir(), ".jasmine", "config.json");
  let cfg;
  try {
    cfg = JSON.parse(readFileSync(path, "utf8"));
  } catch (e) {
    throw new Error(`could not read ${path} (${e.message}); set RELAY_* env vars instead`);
  }
  const p = cfg.provider ?? {};
  const active = (p.profiles ?? []).find((x) => x.id === p.active_id) ?? p.profiles?.[0] ?? p;
  return {
    base_url: env.base_url ?? active.base_url ?? p.base_url,
    api_key: env.api_key ?? active.api_key ?? p.api_key,
    model: env.model ?? active.model ?? p.model,
    name: active.name ?? p.name ?? "relay",
    source: path,
  };
}

function endpoint(base, path) {
  return `${base.replace(/\/+$/, "")}/${path.replace(/^\/+/, "")}`;
}

function dump(label, text) {
  console.log(`\n----- ${label} (${text.length} bytes${text.length > MAX_DUMP ? ", truncated" : ""}) -----`);
  console.log(text.length > MAX_DUMP ? text.slice(0, MAX_DUMP) + "\n…[truncated]…" : text);
  console.log(`----- end ${label} -----`);
}

function parseSse(raw) {
  const out = [];
  for (const block of raw.split(/\r?\n\r?\n/)) {
    const dataLines = block
      .split(/\r?\n/)
      .filter((l) => l.startsWith("data:"))
      .map((l) => l.slice(5).trim());
    if (!dataLines.length) continue;
    const data = dataLines.join("\n");
    if (data === "[DONE]") continue;
    try {
      out.push(JSON.parse(data));
    } catch {
      /* keep-alive / non-JSON */
    }
  }
  return out;
}

function baseBody(cfg, extra) {
  return {
    model: cfg.model,
    messages: [
      {
        role: "user",
        content:
          "What's heavier: 1 kilogram of steel or 1 kilogram of feathers? Reason step by step, then give the answer.",
      },
    ],
    ...extra,
  };
}

async function call(cfg, body) {
  const url = endpoint(cfg.base_url, "chat/completions");
  const res = await fetch(url, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      accept: body.stream ? "text/event-stream" : "application/json",
      ...(cfg.api_key ? { authorization: `Bearer ${cfg.api_key}` } : {}),
    },
    body: JSON.stringify(body),
  });
  return { status: res.status, raw: await res.text(), url };
}

// Look at every delta/message object and tally where reasoning could hide.
function analyzeStream(raw) {
  const a = {
    deltaKeys: new Set(),
    content: 0,
    reasoning_content: 0,
    reasoning: 0,
    reasoning_text: 0,
    thinkTag: false,
    toolCalls: 0,
    finish: new Set(),
  };
  for (const chunk of parseSse(raw)) {
    const c = chunk.choices?.[0];
    if (!c) continue;
    if (c.finish_reason) a.finish.add(c.finish_reason);
    const d = c.delta ?? c.message ?? {};
    for (const k of Object.keys(d)) a.deltaKeys.add(k);
    if (typeof d.content === "string") {
      a.content += d.content.length;
      if (/<\/?think>|<\/?thinking>/i.test(d.content)) a.thinkTag = true;
    }
    for (const f of ["reasoning_content", "reasoning", "reasoning_text"]) {
      if (typeof d[f] === "string") a[f] += d[f].length;
    }
    if (Array.isArray(d.tool_calls)) a.toolCalls += d.tool_calls.length;
  }
  return a;
}

function analyzeSync(raw) {
  let json;
  try {
    json = JSON.parse(raw);
  } catch {
    return { parseError: true };
  }
  const msg = json.choices?.[0]?.message ?? {};
  const content = typeof msg.content === "string" ? msg.content : "";
  return {
    messageKeys: Object.keys(msg),
    reasoningKeys: Object.keys(msg).filter((k) => /reason|think/i.test(k)),
    thinkTag: /<\/?think>|<\/?thinking>/i.test(content),
    finish: json.choices?.[0]?.finish_reason,
    usage: json.usage,
  };
}

async function main() {
  const argv = process.argv.slice(2);
  const useEffort = !argv.includes("--no-effort");
  const cfg = loadConfig();

  console.log("════════ relay reasoning — full process log ════════");
  console.log(`relay:   ${cfg.name}`);
  console.log(`model:   ${cfg.model}        <-- this is what every request below uses`);
  console.log(`base:    ${cfg.base_url}`);
  console.log(`config:  ${cfg.source}`);
  console.log(`api key: ${cfg.api_key ? "present" : "MISSING"}`);
  console.log(`reasoning_effort: ${useEffort ? "medium (sent)" : "omitted (--no-effort)"}`);

  const extra = useEffort ? { reasoning_effort: "medium" } : {};

  // 1) STREAMING request
  console.log("\n\n########## 1. STREAMING (stream: true) ##########");
  const stream = await call(cfg, baseBody(cfg, { ...extra, stream: true }));
  console.log(`HTTP ${stream.status}  ·  POST ${stream.url}`);
  dump("RAW STREAM BODY", stream.raw);
  if (stream.status >= 200 && stream.status < 300) {
    const a = analyzeStream(stream.raw);
    console.log("\n[stream analysis]");
    console.log(`  delta keys:        ${[...a.deltaKeys].join(", ") || "(none)"}`);
    console.log(`  content chars:     ${a.content}`);
    console.log(`  reasoning_content: ${a.reasoning_content}`);
    console.log(`  reasoning:         ${a.reasoning}`);
    console.log(`  reasoning_text:    ${a.reasoning_text}`);
    console.log(`  <think> in content:${a.thinkTag}`);
    console.log(`  tool_calls:        ${a.toolCalls}`);
    console.log(`  finish_reason:     ${[...a.finish].join(", ") || "(none)"}`);
  }

  // 2) NON-STREAMING ("sync") request
  console.log("\n\n########## 2. SYNC (stream: false) ##########");
  const sync = await call(cfg, baseBody(cfg, { ...extra, stream: false }));
  console.log(`HTTP ${sync.status}  ·  POST ${sync.url}`);
  dump("RAW SYNC BODY", sync.raw);
  if (sync.status >= 200 && sync.status < 300) {
    const a = analyzeSync(sync.raw);
    console.log("\n[sync analysis]");
    if (a.parseError) {
      console.log("  (body was not valid JSON)");
    } else {
      console.log(`  message keys:        ${a.messageKeys.join(", ")}`);
      console.log(`  reasoning-ish keys:  ${a.reasoningKeys.join(", ") || "(none)"}`);
      console.log(`  <think> in content:  ${a.thinkTag}`);
      console.log(`  finish_reason:       ${a.finish}`);
      console.log(`  usage:               ${JSON.stringify(a.usage ?? {})}`);
    }
  }

  console.log("\n════════ done — review the raw bodies above to decide the fix ════════");
}

main().catch((e) => {
  console.error(`\n✗ ${e.message}`);
  process.exit(1);
});
