import { convertFileSrc } from "@tauri-apps/api/core";

const JASMINE_PROTOCOL = "jasmine";
const BASE_MARKER = "__jasmine_protocol_base__";
const FALLBACK_BASE = `${JASMINE_PROTOCOL}://localhost`;

type ConvertFileSrc = (filePath: string, protocol?: string) => string;

let cachedBase: string | null = null;

/** Resolve the platform-specific Tauri custom protocol base for Jasmine images. */
export function jasmineProtocolBase(convert: ConvertFileSrc = convertFileSrc): string {
  if (convert === convertFileSrc && cachedBase) return cachedBase;

  const base = resolveProtocolBase(convert);
  if (convert === convertFileSrc) cachedBase = base;
  return base;
}

/** Build a WebView-loadable URL for an in-workspace Board asset. */
export function jasmineUrl(boardId: string, relPath: string): string {
  return buildJasmineUrl(boardId, relPath, jasmineProtocolBase());
}

export function buildJasmineUrl(boardId: string, relPath: string, base: string): string {
  const encBoard = encodeURIComponent(boardId);
  const normalized = relPath.replace(/\\/g, "/").replace(/^\/+/, "");
  const enc = normalized
    .split("/")
    .map((seg) => encodeURIComponent(seg))
    .join("/");
  return `${base.replace(/\/+$/, "")}/${encBoard}/${enc}`;
}

function resolveProtocolBase(convert: ConvertFileSrc): string {
  try {
    // convertFileSrc owns Tauri's platform-specific custom protocol origin
    // rules. We only borrow the origin; Jasmine keeps boardId and relPath as
    // structured path segments so Rust can route and guard them.
    const markerUrl = convert(BASE_MARKER, JASMINE_PROTOCOL);
    const suffix = `/${encodeURIComponent(BASE_MARKER)}`;
    if (markerUrl.endsWith(suffix)) {
      return markerUrl.slice(0, -suffix.length).replace(/\/+$/, "");
    }
  } catch {
    // Browser-only dev shells do not expose Tauri internals. Keep URL creation
    // deterministic so callers still fail at fetch time with a useful URL.
  }
  return FALLBACK_BASE;
}
