import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test } from "@playwright/test";

import { roundAndFlatten } from "@tt/domain";

import { at, createEntry, createTask, drag, login, PASSWORD, startProxy, user } from "./helpers.ts";

test("login lands on the timeline and syncs", async ({ page }) => {
  await login(page, user(1));
  await expect(page).toHaveURL(/\/timeline\/day/);
  await expect(page.getByTestId("empty-day")).toBeVisible();
  await expect(page.getByTestId("sync-status")).toHaveAttribute("data-state", "synced");
});

test("wrong password is refused inline", async ({ page }) => {
  await page.context().setExtraHTTPHeaders({ "x-forwarded-for": "10.0.0.2" });
  await page.goto("/");
  await page.getByLabel("Username").fill(user(2));
  await page.getByRole("textbox", { name: /Password/ }).fill("not-the-password");
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("alert")).toHaveText("Invalid username or password");
});

test("offline reload renders local data and shows Offline", async ({ page, baseURL }) => {
  const proxy = await startProxy(baseURL!);
  try {
    await login(page, user(3), proxy.url);
    await createTask(page, "Works offline +local");
    await expect(page.getByTestId("side-task")).toHaveCount(1);
    await expect(page.getByTestId("sync-status")).toHaveAttribute("data-state", "synced");
    await page.evaluate(() => navigator.serviceWorker.ready);
    proxy.setDown(true);
    await page.reload();
    // Shell from the service worker cache, documents from IndexedDB.
    await expect(page.getByTestId("side-task")).toContainText("Works offline");
    await expect(page.getByTestId("sync-status")).toHaveAttribute("data-state", "offline");
    // Local writes still work offline and are counted as queued.
    await createTask(page, "Queued while offline");
    await expect(page.getByTestId("sync-status")).toContainText(/saved here/);
    proxy.setDown(false);
    await expect(page.getByTestId("sync-status")).toHaveAttribute("data-state", "synced", { timeout: 45_000 });
  } finally {
    await proxy.close();
  }
});

test("two tabs: an entry started in one runs in the other", async ({ page, context }) => {
  await login(page, user(4));
  await createTask(page, "Shared tab task");
  const other = await context.newPage();
  await other.goto("/timeline/day");
  await expect(other.getByTestId("side-task")).toContainText("Shared tab task");
  await page.getByRole("button", { name: "Start Shared tab task" }).click();
  await expect(other.getByTestId("running-chip")).toHaveCount(1, { timeout: 1500 });
  await other.getByRole("button", { name: /Stop all/ }).click();
  await expect(page.getByTestId("running-chip")).toHaveCount(0, { timeout: 1500 });
});

test("drag to create (snapped), move, resize and re-link", async ({ page }) => {
  await login(page, user(5));
  await createTask(page, "Invoice PDF rendering @Billing");
  await createTask(page, "Set up staging DNS @Infra");
  const column = page.getByTestId("day-column").first();

  // 13:07 → 13:52 with a 15 min grid proposes 13:00–14:00.
  await drag(page, await at(column, 13, 7), await at(column, 13, 52));
  await expect(page.getByTestId("create-ghost")).toContainText("13:00 – 14:00");
  await expect(page.getByTestId("task-picker")).toBeVisible();
  await page.keyboard.type("invoice");
  await page.keyboard.press("Enter");
  const block = page.getByTestId("entry-block");
  await expect(block).toContainText("13:00 – 14:00");

  // Move by +30 min.
  await drag(page, await at(column, 13, 20, 300), await at(column, 13, 50, 300));
  await expect(block).toContainText("13:30 – 14:30");
  await expect(page.locator("[data-sonner-toast]").first()).toContainText("Moved");

  // Resize the bottom edge to 15:00.
  const box = (await block.boundingBox())!;
  await drag(page, { x: box.x + 60, y: box.y + box.height + 1 }, await at(column, 15, 0, 300));
  await expect(block).toContainText("13:30 – 15:00");

  // Drop onto the side panel task to re-link; times unchanged.
  const row = page.getByTestId("side-task").filter({ hasText: "Set up staging DNS" });
  const target = (await row.boundingBox())!;
  await drag(page, await at(column, 14, 0, 300), { x: target.x + 80, y: target.y + target.height / 2 });
  await expect(block).toContainText("Set up staging DNS");
  await expect(block).toContainText("13:30 – 15:00");

  // Undo restores the link exactly.
  await page.keyboard.press("Control+z");
  await expect(block).toContainText("Invoice PDF rendering");
});

test("concurrent entries split the column into lanes", async ({ page }) => {
  await login(page, user(6));
  await createTask(page, "Alpha");
  await createTask(page, "Beta");
  await createTask(page, "Gamma");
  await createEntry(page, [9, 0], [11, 0], "alpha");
  // Alpha fills the column, so the others are drawn elsewhere and resized into overlap.
  const column = page.getByTestId("day-column").first();
  const width = (await column.boundingBox())!.width;
  await drag(page, await at(column, 8, 0, width - 10), await at(column, 8, 30, width - 10));
  await page.keyboard.type("beta");
  await page.keyboard.press("Enter");
  // Resize Beta to overlap Alpha: 08:00–10:00.
  const beta = page.getByTestId("entry-block").filter({ hasText: "Beta" });
  const bb = (await beta.boundingBox())!;
  await drag(page, { x: bb.x + 20, y: bb.y + bb.height + 1 }, await at(column, 10, 0, width - 10));
  await drag(page, await at(column, 12, 0, width - 10), await at(column, 12, 30, width - 10));
  await page.keyboard.type("gamma");
  await page.keyboard.press("Enter");
  const gamma = page.getByTestId("entry-block").filter({ hasText: "Gamma" });
  const gb = (await gamma.boundingBox())!;
  await drag(page, { x: gb.x + gb.width - 10, y: gb.y + 3 }, await at(column, 9, 30, width - 10));
  await expect(page.getByTestId("entry-block")).toHaveCount(3);
  for (const name of ["Alpha", "Beta", "Gamma"]) {
    await expect(page.getByTestId("entry-block").filter({ hasText: name })).toHaveAttribute("data-lanes", "3");
  }
  const widths = await page.getByTestId("entry-block").evaluateAll((els) => els.map((el) => el.getBoundingClientRect().width));
  expect(Math.max(...widths) - Math.min(...widths)).toBeLessThan(2);
  // Totals: summed minus wall-clock equals overlap.
  const summed = await page.getByTestId("total-summed").innerText();
  const wall = await page.getByTestId("total-wall").innerText();
  expect(summed).not.toBe(wall);
});

test("renaming a task renames its entries on the timeline", async ({ page, context }) => {
  await login(page, user(7));
  await createTask(page, "Old title");
  await createEntry(page, [10, 0], [11, 0], "old");
  const timeline = await context.newPage();
  await timeline.goto("/timeline/day");
  await expect(timeline.getByTestId("entry-block")).toContainText("Old title");
  await page.goto("/tasks/1");
  await page.getByTestId("task-title").click();
  await page.getByLabel("Title").fill("New title");
  await page.keyboard.press("Enter");
  await expect(timeline.getByTestId("entry-block")).toContainText("New title");
});

test("export preview equals the shared round + flatten output", async ({ page }) => {
  await login(page, user(8));
  await createTask(page, "Billing call");
  // Alt frees the snap: 13:00–13:05 and 13:10–13:45.
  await createEntry(page, [13, 0], [13, 5], "billing", true);
  await createEntry(page, [13, 10], [13, 45], "billing", true);
  await page.goto("/reports");
  await page.getByRole("radiogroup", { name: "Range" }).getByRole("radio", { name: "Day" }).click();
  const raw = await page.getByTestId("raw-row").evaluateAll((rows) => rows.map((r) => [Number(r.getAttribute("data-start")), Number(r.getAttribute("data-end"))]));
  const flat = await page.getByTestId("flat-row").evaluateAll((rows) => rows.map((r) => [Number(r.getAttribute("data-start")), Number(r.getAttribute("data-end"))]));
  expect(raw).toHaveLength(2);
  const expected = roundAndFlatten(
    raw.map(([start, end], i) => ({ task: "t", start: start!, end: end!, entries: [String(i)] })),
    15 * 60,
    "up",
    "entry",
    "Europe/Berlin",
  ).map((r) => [r.start, r.end]);
  expect(flat).toEqual(expected);
  const texts = await page.getByTestId("flat-row").allInnerTexts();
  expect(texts[0]).toContain("13:00–13:15");
  expect(texts[1]).toContain("13:15–14:00");
});

test("logout shows unsynced changes before wiping", async ({ page, baseURL }) => {
  const proxy = await startProxy(baseURL!);
  try {
    await login(page, user(9), proxy.url);
    await createTask(page, "Synced task");
    await expect(page.getByTestId("sync-status")).toHaveAttribute("data-state", "synced");
    proxy.setDown(true);
    await createTask(page, "Unsynced task");
    await expect(page.getByTestId("sync-status")).toContainText(/saved here/);
    await page.getByRole("link", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Log out and wipe local data" }).click();
    await expect(page.getByTestId("logout-unsynced")).toContainText(/\d+ unsynced change/);
    await page.getByRole("button", { name: /Discard .* and log out/ }).click();
    await expect(page.getByRole("button", { name: /Sign in/ })).toBeVisible();
    // The worker reopens an empty repo for the next sign-in: no documents remain.
    const stored = await page.evaluate(
      () =>
        new Promise<number>((resolve) => {
          const open = indexedDB.open("tt");
          open.onsuccess = () => {
            const db = open.result;
            if (!db.objectStoreNames.contains("documents")) return resolve(0);
            const keys = db.transaction("documents").objectStore("documents").getAllKeys();
            // automerge-repo keeps its own storage id; anything else is a document chunk.
            keys.onsuccess = () => resolve(keys.result.filter((k) => (k as string[])[0] !== "storage-adapter-id").length);
          };
          open.onerror = () => resolve(-1);
        }),
    );
    expect(stored).toBe(0);
  } finally {
    await proxy.close();
  }
});

// ---------- daemon-created data ----------

const TT = fileURLToPath(new URL("../../target/debug/tt", import.meta.url));

function ttEnv(home: string) {
  return {
    ...process.env,
    HOME: home,
    XDG_CONFIG_HOME: join(home, "config"),
    XDG_DATA_HOME: join(home, "data"),
    XDG_STATE_HOME: join(home, "state"),
    XDG_RUNTIME_DIR: join(home, "run"),
  };
}

function tt(home: string, args: string[], input?: string) {
  const result = spawnSync(TT, args, { env: ttEnv(home), input, encoding: "utf8", timeout: 20_000 });
  if (result.status !== 0) throw new Error(`tt ${args.join(" ")}: ${result.stderr}`);
  return result.stdout;
}

test("data created by the daemon and CLI shows up in the web app", async ({ page, baseURL }) => {
  const name = user(10);
  const home = mkdtempSync(join(tmpdir(), "tt-web-daemon-"));
  spawnSync("mkdir", ["-p", join(home, "run")]);
  // A foreground daemon of our own (an auto-started one would hold our pipes).
  const daemon = spawn(TT, ["daemon"], { env: ttEnv(home), stdio: "ignore" });
  await expect.poll(() => spawnSync(TT, ["status"], { env: ttEnv(home) }).status, { timeout: 10_000 }).toBe(0);
  try {
    tt(home, ["login", baseURL!, "--username", name], `${PASSWORD}\n`);
    tt(home, ["task", "add", "From the CLI", "+cli", "@Daemon", "ticket:CLI-1"]);
    tt(home, ["start", "1"]);
    await login(page, name);
    await expect(page.getByTestId("running-chip")).toContainText("From the CLI", { timeout: 20_000 });
    // And back: a stop in the browser reaches the daemon.
    await page.getByRole("button", { name: /Stop all/ }).click();
    await expect
      .poll(() => JSON.parse(tt(home, ["-j", "status"])).workspace?.running?.length ?? -1, { timeout: 20_000 })
      .toBe(0);
  } finally {
    daemon.kill();
  }
});
