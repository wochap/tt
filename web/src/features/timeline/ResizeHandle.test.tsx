import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { ResizeHandle } from "./EntryBlock.tsx";

afterEach(cleanup);

const hitOf = (height: number) => {
  render(<ResizeHandle edge="bottom" handleRef={() => {}} height={height} color="red" label="Resize end" />);
  return screen.getByLabelText("Resize end").style.getPropertyValue("--hit");
};

describe("ResizeHandle hit area", () => {
  it("is capped at 44 px on tall blocks", () => {
    expect(hitOf(300)).toBe("min(44px, 100px)");
  });

  it("is clamped to a third of a short block (15 min at 56 px/h) so the body stays draggable", () => {
    const height = 14;
    expect(hitOf(height)).toBe(`min(44px, ${height / 3}px)`);
    // Each handle reaches hit/2 into the block: the two together take a third of it.
    expect(2 * (height / 3 / 2)).toBeLessThan(height);
  });
});
