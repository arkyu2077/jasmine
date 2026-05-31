import { describe, expect, it } from "vitest";
import { normalizeProviderBaseUrl, providerApiKeyLooksLikeUrl } from "./provider";

describe("normalizeProviderBaseUrl", () => {
  it("adds /v1 when the user enters a provider root URL", () => {
    expect(normalizeProviderBaseUrl("https://api.modelsrouter.com")).toBe("https://api.modelsrouter.com/v1");
  });

  it("adds /v1 after an existing API path", () => {
    expect(normalizeProviderBaseUrl("https://example.com/openai/")).toBe("https://example.com/openai/v1");
  });

  it("keeps URLs that already contain a v1 path segment", () => {
    expect(normalizeProviderBaseUrl("https://openrouter.ai/api/v1/")).toBe("https://openrouter.ai/api/v1");
  });

  it("does not treat v10 as v1", () => {
    expect(normalizeProviderBaseUrl("https://example.com/v10")).toBe("https://example.com/v10/v1");
  });
});

describe("providerApiKeyLooksLikeUrl", () => {
  it("detects a provider URL pasted into the API key field", () => {
    expect(providerApiKeyLooksLikeUrl("https://api.modelsrouter.com")).toBe(true);
  });

  it("accepts normal bearer-token shaped keys", () => {
    expect(providerApiKeyLooksLikeUrl("sk-test-key")).toBe(false);
  });
});
