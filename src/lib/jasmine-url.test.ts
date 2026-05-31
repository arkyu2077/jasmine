import { describe, expect, it } from "vitest";
import { buildJasmineUrl, jasmineProtocolBase } from "./jasmine-url";

describe("jasmineProtocolBase", () => {
  it("uses Tauri's Windows custom protocol shape", () => {
    const base = jasmineProtocolBase((path, protocol) => `http://${protocol}.localhost/${encodeURIComponent(path)}`);

    expect(base).toBe("http://jasmine.localhost");
  });

  it("uses native custom protocol shape on WebKit-style platforms", () => {
    const base = jasmineProtocolBase((path, protocol) => `${protocol}://localhost/${encodeURIComponent(path)}`);

    expect(base).toBe("jasmine://localhost");
  });

  it("falls back to the native custom protocol when Tauri internals are unavailable", () => {
    const base = jasmineProtocolBase(() => {
      throw new Error("missing Tauri internals");
    });

    expect(base).toBe("jasmine://localhost");
  });
});

describe("buildJasmineUrl", () => {
  it("keeps board id in the path and encodes each relative path segment", () => {
    const url = buildJasmineUrl("board 1", "imports/cute tiger#1.png", "http://jasmine.localhost");

    expect(url).toBe("http://jasmine.localhost/board%201/imports/cute%20tiger%231.png");
  });

  it("normalizes Windows separators without encoding path slashes", () => {
    const url = buildJasmineUrl("board-1", String.raw`renders\final image.png`, "jasmine://localhost/");

    expect(url).toBe("jasmine://localhost/board-1/renders/final%20image.png");
  });

  it("strips leading slashes from workspace-relative paths", () => {
    const url = buildJasmineUrl("board-1", "/imports/a.png", "http://jasmine.localhost");

    expect(url).toBe("http://jasmine.localhost/board-1/imports/a.png");
  });
});
