import { createServer, connect, type Socket } from "node:net";

import { type Locator, type Page, expect } from "@playwright/test";

export const PASSWORD = "password123";

/** Server account `n` (e2e/serve.sh creates e2e1 … e2e16); one per test. */
export function user(n: number): string {
  return `e2e${n}`;
}

export async function login(page: Page, name: string, base = ""): Promise<void> {
  // One client address per account: the server rate-limits logins per address.
  await page.context().setExtraHTTPHeaders({ "x-forwarded-for": `10.0.0.${name.replace(/\D/g, "")}` });
  await page.goto(`${base}/`);
  await page.getByLabel("Username").fill(name);
  await page.getByRole("textbox", { name: /Password/ }).fill(PASSWORD);
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByTestId("sync-status")).toHaveAttribute("data-state", /synced|syncing/);
}

/** Creates a task through the quick-create palette (C). */
export async function createTask(page: Page, line: string, start = false): Promise<void> {
  await page.keyboard.press("Escape");
  await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => undefined);
  await page.keyboard.press("c");
  await expect(page.getByRole("combobox")).toBeFocused();
  await page.keyboard.type(line);
  await page.keyboard.press(start ? "Control+Enter" : "Enter");
}

/** Pointer position of local wall time `h:m` in a timeline column. */
export async function at(column: Locator, h: number, m: number, x?: number) {
  const grid = column.page().getByTestId("timeline-grid");
  const firstHour = Number(await grid.getAttribute("data-first-hour"));
  const pxPerHour = Number(await grid.getAttribute("data-px-per-hour"));
  const box = (await column.boundingBox())!;
  // Default: right of the empty-state actions, left of any later lanes.
  return { x: box.x + (x ?? box.width / 2), y: box.y + (h - firstHour + m / 60) * pxPerHour };
}

export interface Proxy {
  url: string;
  /** Drops every open connection and refuses new ones (the server "goes away"). */
  setDown(down: boolean): void;
  close(): Promise<void>;
}

/**
 * A TCP proxy in front of tt-server. Browser network emulation does not reach
 * shared workers, so tests take the server away here instead.
 */
export async function startProxy(target: string): Promise<Proxy> {
  const upstream = new URL(target);
  const sockets = new Set<Socket>();
  let down = false;
  const server = createServer((client) => {
    if (down) return void client.destroy();
    const remote = connect(Number(upstream.port), upstream.hostname);
    sockets.add(client).add(remote);
    const drop = () => {
      client.destroy();
      remote.destroy();
      sockets.delete(client);
      sockets.delete(remote);
    };
    client.on("error", drop).on("close", drop);
    remote.on("error", drop).on("close", drop);
    client.pipe(remote).pipe(client);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = (server.address() as { port: number }).port;
  return {
    url: `http://127.0.0.1:${port}`,
    setDown(next) {
      down = next;
      if (next) for (const socket of sockets) socket.destroy();
    },
    close: () => {
      for (const socket of sockets) socket.destroy();
      return new Promise((resolve) => server.close(() => resolve()));
    },
  };
}

export async function drag(page: Page, from: { x: number; y: number }, to: { x: number; y: number }, modifiers: ("Alt" | "Shift")[] = []) {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  for (const key of modifiers) await page.keyboard.down(key);
  await page.mouse.move(from.x, from.y + (to.y > from.y ? 8 : -8), { steps: 2 });
  await page.mouse.move(to.x, to.y, { steps: 10 });
  await page.mouse.up();
  for (const key of modifiers) await page.keyboard.up(key);
}

/** Drag-creates an entry on the day view and links it through the picker. */
export async function createEntry(page: Page, from: [number, number], to: [number, number], query: string, alt = false) {
  const column = page.getByTestId("day-column").first();
  await drag(page, await at(column, ...from), await at(column, ...to), alt ? ["Alt"] : []);
  await expect(page.getByTestId("task-picker")).toBeVisible();
  await page.keyboard.type(query);
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("task-picker")).toBeHidden();
}
