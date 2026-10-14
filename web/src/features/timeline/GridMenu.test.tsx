import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";

import type { Grids } from "@/lib/grids";

import { GridMenu, sameGrids } from "./GridMenu.tsx";

afterEach(cleanup);

const DEFAULTS: Grids = { start: 15, duration: 15 };

function Harness() {
  const [override, setOverride] = useState<Grids | null>(null);
  return <GridMenu value={override ?? DEFAULTS} defaults={DEFAULTS} onChange={(next) => setOverride(sameGrids(next, DEFAULTS) ? null : next)} />;
}

describe("GridMenu", () => {
  it("shows the defaults, overrides for the session and resets", async () => {
    render(<Harness />);
    const chip = screen.getByTestId("grid-menu");
    expect(chip).toHaveTextContent("Start 15m · Dur 15m");
    expect(screen.queryByTestId("grid-overridden")).toBeNull();

    await userEvent.click(chip);
    for (const name of [/^10 min/, /^30 min/, /^60 min/]) {
      expect(screen.getAllByRole("menuitemradio", { name })[0]).toHaveAttribute("data-disabled");
    }
    await userEvent.click(screen.getAllByRole("menuitemradio", { name: "5 min" })[0]!);
    expect(chip).toHaveTextContent("Start 5m · Dur 15m");
    expect(screen.getByTestId("grid-overridden")).toBeInTheDocument();

    await userEvent.click(chip);
    await userEvent.click(screen.getByRole("menuitem", { name: /Reset to default/ }));
    expect(chip).toHaveTextContent("Start 15m · Dur 15m");
    expect(screen.queryByTestId("grid-overridden")).toBeNull();
  });

  it("drops the start to the largest valid value on a duration change", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByTestId("grid-menu"));
    // Second "10 min" radio is the Duration group's.
    await userEvent.click(screen.getAllByRole("menuitemradio", { name: "10 min" }).at(-1)!);
    expect(screen.getByTestId("grid-menu")).toHaveTextContent("Start 10m · Dur 10m");
  });
});
