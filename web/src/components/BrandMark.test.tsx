import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { BrandMark } from "./BrandMark.tsx";

describe("BrandMark", () => {
  afterEach(cleanup);

  it.each([
    [18, 3],
    [32, 3],
    [64, 4],
  ])("size %i draws %i blocks in theme colors", (size, count) => {
    const { container } = render(<BrandMark size={size} />);
    const blocks = container.querySelectorAll("[data-block]");
    expect(blocks).toHaveLength(count);
    for (const b of blocks) expect(b.getAttribute("fill")).toMatch(/^var\(--ctp-/);
    expect(container.querySelector("[data-tile]")!.getAttribute("fill")).toBe("var(--c-base)");
  });

  it("has no tile outline at small sizes", () => {
    const { container } = render(<BrandMark size={22} />);
    expect(container.querySelector("[data-tile]")!.hasAttribute("stroke")).toBe(false);
  });

  it("is an image named tt", () => {
    render(<BrandMark size={22} />);
    expect(screen.getByRole("img", { name: "tt" })).toHaveAttribute("width", "22");
  });
});
