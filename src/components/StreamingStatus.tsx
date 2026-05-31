import { useEffect, useMemo, useState } from "react";
import { ChevronDown, Loader2, WifiOff } from "lucide-react";
import { useChatStore, type ChatMessage, type TransportStatus, type TurnMonitorEvent } from "../store/chat";
import { useSettingsStore } from "../store/settings";
import { useLocaleStore } from "../i18n/locale";
import { pickPhrase, fixedPhrase, type PhraseLocale } from "../lib/streamingPhrases";

const PHRASE_ROTATE_MS = 12_000;
const STALE_SOFT_MS = 30_000;
const STALE_HARD_MS = 90_000;

export function StreamingStatus() {
  const turnStatus = useChatStore((s) => s.turnStatus);
  const sessionStatus = useChatStore((s) => s.sessionStatus);
  const transportStatus = useChatStore((s) => s.transportStatus);
  const startedAt = useChatStore((s) => s.turnStartedAt);
  const lastActivityAt = useChatStore((s) => s.turnLastActivityAt);
  const monitorEvents = useChatStore((s) => s.turnMonitorEvents);
  const lastBlock = useChatStore((s) => {
    const m = lastAssistant(s.messages);
    return m && m.blocks.length > 0 ? m.blocks[m.blocks.length - 1] : null;
  });
  const blockCount = useChatStore((s) => {
    const m = lastAssistant(s.messages);
    return m ? m.blocks.length : 0;
  });
  const lang = useLocaleStore((s) => s.lang);
  const locale: PhraseLocale = lang === "zh" ? "zh" : "en";
  const providerLabel = useActiveProviderLabel();
  const [expanded, setExpanded] = useState(false);
  const [seed, setSeed] = useState(() => Math.floor(Math.random() * 1000));
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (turnStatus === "running") setSeed((s) => s + 1);
  }, [blockCount, turnStatus]);

  useEffect(() => {
    if (turnStatus !== "running") return;
    const id = setInterval(() => setSeed((s) => s + 1), PHRASE_ROTATE_MS);
    return () => clearInterval(id);
  }, [turnStatus]);

  useEffect(() => {
    if (turnStatus !== "running") return;
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [turnStatus]);

  useEffect(() => {
    if (turnStatus !== "running") setExpanded(false);
  }, [turnStatus]);

  const elapsedMs = startedAt ? Math.max(0, now - startedAt) : 0;
  const idleMs = lastActivityAt ? Math.max(0, now - lastActivityAt) : elapsedMs;
  const phrase = useMemo(() => {
    if (transportStatus) return transportPhrase(locale, transportStatus);
    if (idleMs >= STALE_HARD_MS) return hardWaitPhrase(locale);
    if (idleMs >= STALE_SOFT_MS) return softWaitPhrase(locale, providerLabel);
    if (sessionStatus === "starting") return fixedPhrase(locale, "starting");
    if (!lastBlock) return pickPhrase(locale, "general", seed);
    if (lastBlock.type === "thinking" && lastBlock.active) return fixedPhrase(locale, "thinking");
    if (lastBlock.type === "tool" && lastBlock.status === "running") return fixedPhrase(locale, "tool", lastBlock.name);
    if (lastBlock.type === "image" && lastBlock.status === "generating") return pickPhrase(locale, "image", seed);
    return pickPhrase(locale, "general", seed);
  }, [locale, transportStatus, idleMs, providerLabel, sessionStatus, lastBlock, seed]);

  if (turnStatus !== "running") return null;

  const isAttention = Boolean(transportStatus) || idleMs >= STALE_SOFT_MS;
  const detail = detailLine(locale, elapsedMs, idleMs, providerLabel);
  const events = monitorEvents.slice().reverse();

  return (
    <div
      className={`cm-streaming-status${isAttention ? " cm-streaming-status--transport" : ""}`}
      role="status"
      aria-live="polite"
    >
      <div className="cm-streaming-status__main">
        {isAttention ? (
          <WifiOff size={13} className="cm-streaming-status__transport-ico" />
        ) : (
          <Loader2 size={13} className="cm-streaming-status__spin" />
        )}
        <div className="cm-streaming-status__copy">
          <span className="cm-streaming-status__text">{phrase}</span>
          <span className="cm-streaming-status__elapsed">{detail}</span>
        </div>
        <button
          type="button"
          className={`cm-streaming-status__toggle${expanded ? " is-open" : ""}`}
          aria-expanded={expanded}
          onClick={() => setExpanded((v) => !v)}
        >
          {locale === "zh" ? "明细" : "Details"}
          <ChevronDown size={13} />
        </button>
      </div>
      {expanded && events.length > 0 && (
        <ol className="cm-streaming-status__events">
          {events.map((event) => (
            <li key={event.id} className={`cm-streaming-status__event cm-streaming-status__event--${event.level ?? "info"}`}>
              <time>{formatClock(event.at)}</time>
              <span>{eventLabel(locale, event, providerLabel)}</span>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

function transportPhrase(locale: PhraseLocale, status: TransportStatus): string {
  if (status.phase === "fallback") {
    return locale === "zh" ? "信号有点抖，换条路继续…" : "Connection wobbled, trying another path…";
  }
  const suffix = status.attempt && status.max ? ` ${status.attempt}/${status.max}` : "";
  return locale === "zh" ? `信号有点抖，正在重连${suffix}…` : `Connection wobbled, reconnecting${suffix}…`;
}

function softWaitPhrase(locale: PhraseLocale, provider: string): string {
  return locale === "zh"
    ? `灵感咖啡正在泡，等 ${provider} 回音…`
    : `Brewing inspiration, waiting for ${provider}…`;
}

function hardWaitPhrase(locale: PhraseLocale): string {
  return locale === "zh"
    ? "这杯咖啡泡得有点久，可以继续等或点停止"
    : "This is taking a while. You can keep waiting or stop it.";
}

function detailLine(locale: PhraseLocale, elapsedMs: number, idleMs: number, provider: string): string {
  const elapsed = formatDuration(locale, Math.floor(elapsedMs / 1000));
  const idle = formatDuration(locale, Math.floor(idleMs / 1000));
  if (locale === "zh") return `已运行 ${elapsed} · 最近动静 ${idle} 前 · ${provider}`;
  return `Running ${elapsed} · last update ${idle} ago · ${provider}`;
}

function eventLabel(locale: PhraseLocale, event: TurnMonitorEvent, provider: string): string {
  const detail = compactDetail(event.detail);
  if (locale === "zh") {
    switch (event.kind) {
      case "submitted":
        return detail ? `已送到 ${provider} · ${detail} 张参考图` : `已送到 ${provider}`;
      case "thinking":
        return "脑内分镜开始排队";
      case "tool":
        return detail ? `请工具上场：${detail}` : "请工具上场";
      case "image_start":
        return "开始生成图片";
      case "image_done":
        return "图片已放到画布";
      case "reconnecting":
        return detail ? `连接有点抖，重连 ${detail}` : "连接有点抖，正在重连";
      case "fallback":
        return "连接不稳，切换通道继续";
      case "warning":
        return detail === "runtime_notice" ? "收到一条系统提醒，已收起" : `提醒：${detail}`;
      case "error":
        return detail ? `出错：${detail}` : "出错了";
    }
  }
  switch (event.kind) {
    case "submitted":
      return detail ? `Sent to ${provider} · ${detail} reference image(s)` : `Sent to ${provider}`;
    case "thinking":
      return "Storyboards lining up";
    case "tool":
      return detail ? `Tool joined in: ${detail}` : "Tool joined in";
    case "image_start":
      return "Image generation started";
    case "image_done":
      return "Image placed on canvas";
    case "reconnecting":
      return detail ? `Connection wobbled, reconnecting ${detail}` : "Connection wobbled, reconnecting";
    case "fallback":
      return "Connection unstable, continuing through fallback";
    case "warning":
      return detail === "runtime_notice" ? "System notice tucked away" : `Notice: ${detail}`;
    case "error":
      return detail ? `Error: ${detail}` : "Something failed";
  }
}

function lastAssistant(messages: ChatMessage[]): Extract<ChatMessage, { role: "assistant" }> | null {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role === "assistant") return m;
  }
  return null;
}

function formatDuration(locale: PhraseLocale, sec: number): string {
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  if (locale === "zh") {
    if (h > 0) return `${h}小时${m}分${s}秒`;
    if (m > 0) return `${m}分${s}秒`;
    return `${s}秒`;
  }
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

function formatClock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function compactDetail(detail: string | null | undefined): string | null {
  if (!detail) return null;
  return detail.length > 72 ? `${detail.slice(0, 69)}…` : detail;
}

function useActiveProviderLabel(): string {
  const provider = useSettingsStore((s) => s.config.provider);
  return useMemo(() => {
    if (!provider.enabled) return "Codex";
    const active = provider.profiles?.find((profile) => profile.id === provider.active_id);
    return (active?.name || provider.name || "Provider").trim() || "Provider";
  }, [provider]);
}
