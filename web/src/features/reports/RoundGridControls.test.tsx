import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { defaultStartMinutes, type RoundGrids, RoundGridControls } from "./ReportsPage.tsx";

function Harness({ initial }: { initial: RoundGrids }) {
  const [value, setValue] = useState(initial);
  return <RoundGridControls value={value} onChange={setValue} />;
}

afterEach(cleanup);

describe("round grid controls", () => {
  it("defaults the start to the largest valid value ≤ 15 min", () => {
    expect(defaultStartMinutes(15)).toBe(15);
    expect(defaultStartMinutes(30)).toBe(15);
    expect(defaultStartMinutes(10)).toBe(10);
    expect(defaultStartMinutes(5)).toBe(5);
  });

  it("disables start grids that do not divide the duration", async () => {
    const user = userEvent.setup();
    render(<Harness initial={{ grid: 15, mode: "up", startGrid: 15, startMode: "up" }} />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    const item = (name: string) => screen.getByRole("menuitemradio", { name });
    for (const name of ["10 min", "30 min", "60 min"]) expect(item(name)).toHaveAttribute("data-disabled");
    for (const name of ["off", "5 min", "15 min"]) expect(item(name)).not.toHaveAttribute("data-disabled");
  });

  it("falls back from 15 to 10 min when the duration becomes 10 min", async () => {
    const user = userEvent.setup();
    render(<Harness initial={{ grid: 15, mode: "up", startGrid: 15, startMode: "up" }} />);
    await user.click(screen.getByRole("button", { name: "Duration" }));
    await user.click(screen.getByRole("menuitemradio", { name: "10 min" }));
    expect(screen.getByRole("button", { name: "Start" })).toHaveTextContent("10 min");
  });
});
