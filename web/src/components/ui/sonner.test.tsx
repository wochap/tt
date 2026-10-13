import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { render, screen } from "@testing-library/react";
import { toast } from "sonner";
import { describe, expect, it } from "vitest";

import { Toaster } from "./sonner.tsx";

const css = readFileSync(resolve(process.cwd(), "src/styles.css"), "utf8");

describe("undo toast countdown", () => {
  it("renders an undo toast with an action to attach the bar to", async () => {
    render(<Toaster />);
    toast("Deleted 5 entries", { action: { label: "Undo", onClick: () => {} } });
    const action = await screen.findByText("Undo");
    expect(action.closest("[data-sonner-toast]")).not.toBeNull();
  });

  it("drains over 8 s and pauses on hover and focus", () => {
    expect(css).toMatch(/\[data-sonner-toast\]:has\(\[data-action\]\)::after\s*{[^}]*animation:\s*toast-drain 8s linear/);
    expect(css).toMatch(/:hover::after,\s*\[data-sonner-toast\]:focus-within::after\s*{[^}]*animation-play-state:\s*paused/);
    expect(css).toMatch(/height:\s*2px/);
  });
});
