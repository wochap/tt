// Contrast audit of small text (design frame 13): every text token must reach
// 4.5:1 on base and mantle in both flavors, read straight from styles.css.

import { readFileSync } from "node:fs";
import { resolve as resolvePath } from "node:path";

import { describe, expect, it } from "vitest";

// The test runs from web/ (vitest root).
const css = readFileSync(resolvePath(process.cwd(), "src/styles.css"), "utf8");

function block(selector: string): Record<string, string> {
  const start = css.indexOf(selector);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  const vars: Record<string, string> = {};
  for (const m of body.matchAll(/(--[\w-]+):\s*([^;]+);/g)) vars[m[1]!] = m[2]!.trim();
  return vars;
}

function resolve(vars: Record<string, string>, name: string): string {
  let value = vars[name]!;
  for (let i = 0; i < 5 && value?.startsWith("var("); i++) value = vars[value.slice(4, -1)]!;
  return value;
}

function luminance(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0]! + 0.7152 * c[1]! + 0.0722 * c[2]!;
}

function ratio(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi! + 0.05) / (lo! + 0.05);
}

const flavors = { mocha: block(':root,\n[data-theme="mocha"]'), latte: block('[data-theme="latte"]') };
const TEXT = ["--c-text", "--c-sub1", "--c-muted", "--c-faint", "--c-link"];
const COLORED = ["red", "peach", "yellow", "green", "teal", "sapphire", "blue", "mauve", "lavender", "pink", "maroon", "sky", "flamingo", "rosewater"].map((c) => `--fg-${c}`);

describe.each(Object.entries(flavors))("%s small text", (_name, vars) => {
  const grounds = ["--c-base", "--c-mantle"].map((g) => resolve(vars, g));
  it.each([...TEXT, ...COLORED])("%s ≥ 4.5:1 on base and mantle", (token) => {
    const fg = resolve(vars, token);
    for (const bg of grounds) expect(ratio(fg, bg)).toBeGreaterThanOrEqual(4.5);
  });

  it("faint is a distinct step from muted", () => {
    expect(resolve(vars, "--c-faint")).not.toBe(resolve(vars, "--c-muted"));
  });
});
