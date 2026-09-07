import { describe, expect, test } from "bun:test";
import {
  isHttpUrl,
  isSkippableCt,
  looksLikeJson,
  scanForMediaUrls,
  sniffManifestMagic,
} from "./media-sniff";

describe("isHttpUrl", () => {
  const cases: Array<[string, boolean]> = [
    ["http://example.com/a.mp4", true],
    ["https://example.com/a.mp4", true],
    ["blob:https://example.com/uuid", false],
    ["data:video/mp4;base64,AAAA", false],
    ["ftp://example.com/a.mp4", false],
    ["", false],
  ];
  for (const [url, expected] of cases) {
    test(`${JSON.stringify(url)} -> ${expected}`, () => {
      expect(isHttpUrl(url)).toBe(expected);
    });
  }
});

describe("isSkippableCt", () => {
  const skippable: string[] = [
    "image/png",
    "font/woff2",
    "text/css",
    "application/javascript",
  ];
  for (const ct of skippable) {
    test(`${ct} -> true`, () => {
      expect(isSkippableCt(ct)).toBe(true);
    });
  }

  const notSkippable: string[] = [
    "video/mp4",
    "application/json",
    "application/vnd.apple.mpegurl",
    "",
  ];
  for (const ct of notSkippable) {
    test(`${JSON.stringify(ct)} -> false`, () => {
      expect(isSkippableCt(ct)).toBe(false);
    });
  }
});

describe("sniffManifestMagic", () => {
  test("HLS manifest with trailing content", () => {
    expect(sniffManifestMagic("#EXTM3U\n#EXT-X-VERSION:3\n")).toBe(
      "hls-manifest",
    );
  });

  test("leading whitespace before #EXTM3U is trimmed", () => {
    expect(sniffManifestMagic("  \n#EXTM3U")).toBe("hls-manifest");
  });

  test("lowercase #extm3u is normalized to uppercase match", () => {
    expect(sniffManifestMagic("#extm3u\n#ext-x-version:3\n")).toBe(
      "hls-manifest",
    );
  });

  test("DASH manifest via <MPD within first 300 chars", () => {
    expect(
      sniffManifestMagic('<?xml version="1.0"?><MPD xmlns="urn:mpeg:dash">'),
    ).toBe("dash-manifest");
  });

  test("plain HTML is not a manifest", () => {
    expect(sniffManifestMagic("<!doctype html><html></html>")).toBe(null);
  });

  test("JSON body is not a manifest", () => {
    expect(sniffManifestMagic('{"a":1}')).toBe(null);
  });

  test("empty string is not a manifest", () => {
    expect(sniffManifestMagic("")).toBe(null);
  });

  test("whitespace-only string is not a manifest", () => {
    expect(sniffManifestMagic("   \n\t  ")).toBe(null);
  });

  test("#EXTM3U not at the start of the body is not detected (strict prefix)", () => {
    const text = `${"x".repeat(500)}#EXTM3U\n#EXT-X-VERSION:3\n`;
    expect(sniffManifestMagic(text)).toBe(null);
  });
});

describe("looksLikeJson", () => {
  test("content-type carrying json wins regardless of body", () => {
    expect(looksLikeJson("application/json", "irrelevant body")).toBe(true);
  });

  test("body starting with { is JSON-like even with unrelated content-type", () => {
    expect(looksLikeJson("text/plain", '  {"a":1}')).toBe(true);
  });

  test("body starting with [ is JSON-like even with unrelated content-type", () => {
    expect(looksLikeJson("text/plain", "[1,2]")).toBe(true);
  });

  test("HTML body with HTML content-type is not JSON-like", () => {
    expect(looksLikeJson("text/html", "<html></html>")).toBe(false);
  });
});

describe("scanForMediaUrls", () => {
  test("extracts a streaming URL nested inside an object", () => {
    const result = scanForMediaUrls(
      { d: { hls: "https://cdn/x/i.m3u8?sig=1" } },
      "https://p/",
    );
    expect(result).toEqual(["https://cdn/x/i.m3u8?sig=1"]);
  });

  test("excludes image URLs", () => {
    const result = scanForMediaUrls({ img: "https://x/a.png" }, "https://p/");
    expect(result).toEqual([]);
  });

  test("resolves relative URLs against baseUrl (root-relative path)", () => {
    const result = scanForMediaUrls(
      { u: "/live/i.m3u8" },
      "https://h/p/api",
    );
    expect(result).toEqual(["https://h/live/i.m3u8"]);
  });

  test("drops bare filenames (no path separator) instead of fabricating sibling URLs", () => {
    // 官网 /api/release 的 assets[].name 是裸文件名；旧实现把它相对解析成
    // https://fluxdown.zerx.dev/api/<file>（不存在，404）并污染资源面板。
    const result = scanForMediaUrls(
      { name: "FluxDown-0.1.57-android-universal.apk" },
      "https://fluxdown.zerx.dev/api/release",
    );
    expect(result).toEqual([]);
  });

  test("keeps directory-relative references that contain a path segment", () => {
    const result = scanForMediaUrls({ u: "hls/i.m3u8" }, "https://h/p/");
    expect(result).toEqual(["https://h/p/hls/i.m3u8"]);
  });

  test("collects non-manifest media URLs too (e.g. .mp4)", () => {
    const result = scanForMediaUrls({ v: "https://x/a.mp4" }, "https://p/");
    expect(result).toEqual(["https://x/a.mp4"]);
  });

  test("deduplicates the same absolute URL found at multiple paths", () => {
    const result = scanForMediaUrls(
      { a: "https://x/i.m3u8", b: { c: "https://x/i.m3u8" } },
      "https://p/",
    );
    expect(result).toEqual(["https://x/i.m3u8"]);
  });

  test("depth budget: a media URL beyond MAX_SCAN_DEPTH is not collected", () => {
    let deep: unknown = "https://x/deep.mp4";
    for (let i = 0; i < 25; i++) deep = { n: deep };
    const result = scanForMediaUrls(deep, "https://p/");
    expect(result).toEqual([]);
  });

  test("depth budget: a media URL within the depth budget is still collected", () => {
    const shallow = { n: { n: { n: { n: { n: "https://x/shallow.mp4" } } } } };
    const result = scanForMediaUrls(shallow, "https://p/");
    expect(result).toEqual(["https://x/shallow.mp4"]);
  });

  test("node budget: a huge flat array does not throw and returns a bounded result", () => {
    const arr: string[] = [];
    for (let i = 0; i < 100_000; i++) arr.push(`https://x/i${i}.mp4`);
    let result: string[] = [];
    expect(() => {
      result = scanForMediaUrls(arr, "https://p/");
    }).not.toThrow();
    // Width guard (MAX_SCAN_NODES = 5000) must cap traversal far below the
    // 100,000-element input; a regression that drops the node budget would
    // let this grow toward 100,000.
    expect(result.length).toBeGreaterThan(0);
    expect(result.length).toBeLessThan(5000);
  });

  test("non-object roots (null / number / plain string) yield an empty array without throwing", () => {
    expect(scanForMediaUrls(null, "https://p/")).toEqual([]);
    expect(scanForMediaUrls(42, "https://p/")).toEqual([]);
    expect(scanForMediaUrls("hello", "https://p/")).toEqual([]);
  });

  test("an unparsable URL string in the tree is skipped, not thrown", () => {
    let result: string[] = [];
    expect(() => {
      result = scanForMediaUrls(
        { bad: "http://exa mple.com/a.mp4", good: "https://x/b.mp4" },
        "https://p/",
      );
    }).not.toThrow();
    expect(result).toEqual(["https://x/b.mp4"]);
  });
});
