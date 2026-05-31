export function isValidProviderBaseUrl(value: string): boolean {
  const v = value.trim();
  if (!v) return false;
  try {
    const url = new URL(v);
    return (url.protocol === "http:" || url.protocol === "https:") && !!url.host;
  } catch {
    return false;
  }
}

export function normalizeProviderBaseUrl(value: string): string {
  const v = value.trim();
  if (!isValidProviderBaseUrl(v)) return v;

  const url = new URL(v);
  url.search = "";
  url.hash = "";

  const segments = url.pathname.split("/").filter(Boolean);
  if (!segments.some((segment) => segment.toLowerCase() === "v1")) {
    const basePath = url.pathname.replace(/\/+$/, "");
    url.pathname = `${basePath}/v1`;
  }

  return url.toString().replace(/\/$/, "");
}

export function providerApiKeyLooksLikeUrl(value: string): boolean {
  const v = value.trim();
  if (!v) return false;
  try {
    const url = new URL(v);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}
