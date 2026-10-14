import { existsSync, readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { pwaIcons } from "../pwa-icons";

// Vitest runs from web/ (jsdom rewrites `import.meta.url`).
const web = `${process.cwd()}/`;
const publicFile = (src: string) => `${web}public${src}`;

// PNG: 8-byte signature, then the IHDR chunk with width and height at offsets 16 and 20.
function pngSize(path: string) {
  const bytes = readFileSync(path);
  return `${bytes.readUInt32BE(16)}x${bytes.readUInt32BE(20)}`;
}

describe("app icons", () => {
  const html = readFileSync(`${web}index.html`, "utf8");
  const linked = [...html.matchAll(/<link rel="(?:icon|apple-touch-icon)" href="([^"]+)"/g)].map((m) => m[1] ?? "");

  it("links the favicon and apple touch icon", () => {
    expect(linked).toEqual(["/favicon.svg", "/apple-touch-icon.png"]);
  });

  it("serves every referenced icon from public/", () => {
    for (const src of [...linked, ...pwaIcons.map((icon) => icon.src)]) {
      expect(existsSync(publicFile(src)), src).toBe(true);
    }
  });

  it("matches each manifest PNG to its declared size", () => {
    for (const icon of pwaIcons.filter((i) => i.type === "image/png")) {
      expect(pngSize(publicFile(icon.src)), icon.src).toBe(icon.sizes);
    }
    expect(pngSize(publicFile("/apple-touch-icon.png"))).toBe("180x180");
  });

  it("draws the mark without a lettermark", () => {
    for (const src of ["/icon.svg", "/favicon.svg"]) {
      expect(readFileSync(publicFile(src), "utf8"), src).not.toContain("<text");
    }
  });
});
