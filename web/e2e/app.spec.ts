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

test("pending range can be moved and resized; empty space replaces it", async ({ page }) => {
  await login(page, user(12));
  const column = page.getByTestId("day-column").first();
  const ghost = page.getByTestId("create-ghost");

  await drag(page, await at(column, 13, 7), await at(column, 13, 52));
  await expect(ghost).toContainText("13:00 – 14:00");
  await expect(page.getByTestId("task-picker")).toBeVisible();

  // Move by +30 min; the picker follows and no new range is created.
  await drag(page, await at(column, 13, 30, 200), await at(column, 14, 0, 200));
  await expect(ghost).toHaveCount(1);
  await expect(ghost).toContainText("13:30 – 14:30");
  await expect(page.getByTestId("task-picker")).toBeVisible();

  // Resize the bottom edge to 15:00.
  const box = (await ghost.boundingBox())!;
  await drag(page, { x: box.x + 60, y: box.y + box.height }, await at(column, 15, 0, 200));
  await expect(ghost).toContainText("13:30 – 15:00");

  // A press on empty space replaces the range.
  await drag(page, await at(column, 17, 0, 200), await at(column, 18, 0, 200));
  await expect(ghost).toHaveCount(1);
  await expect(ghost).toContainText("17:00 – 18:00");
});

test.describe("coarse pointer", () => {
  test.use({ hasTouch: true });

  test("resize handle hit area extends beyond the visible edge", async ({ page }) => {
    await login(page, user(13));
    const column = page.getByTestId("day-column").first();
    await drag(page, await at(column, 17, 0, 200), await at(column, 18, 0, 200));
    const ghost = page.getByTestId("create-ghost");
    const handle = ghost.getByLabel("Resize end");
    const box = (await ghost.boundingBox())!;
    const hit = (await handle.boundingBox())!;
    const coarse = await page.evaluate(() => matchMedia("(pointer: coarse)").matches);
    // Fine pointers keep the 8 px strip; coarse ones get ~44 px centered on the edge.
    expect(hit.height).toBeGreaterThanOrEqual(coarse ? 18 : 8);
    expect(hit.height).toBeLessThanOrEqual(coarse ? 44 : 8);
    expect(Math.abs(hit.y + hit.height / 2 - (box.y + box.height))).toBeLessThanOrEqual(2);
  });
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

test("filter the timeline to a project: dimmed entries, filtered totals, survives week and Back, clears with ⇧F", async ({ page }) => {
  await login(page, user(17));
  await createTask(page, "Atlas job @Atlas");
  await createTask(page, "Billing job @Billing");
  await createEntry(page, [9, 0], [11, 0], "atlas job");
  await createEntry(page, [13, 0], [14, 0], "billing job");
  await expect(page.getByTestId("entry-block")).toHaveCount(2);
  await page.keyboard.press("f");
  await page.keyboard.type("atlas");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/project=/);
  await expect(page.getByTestId("filter-chip")).toContainText("Atlas");
  const billing = page.getByTestId("entry-block").filter({ hasText: "Billing job" });
  await expect(billing).toHaveAttribute("data-dimmed", "true");
  await expect(page.getByTestId("entry-block").filter({ hasText: "Atlas job" })).not.toHaveAttribute("data-dimmed", "true");
  await expect(page.getByTestId("totals-filtered")).toHaveText(/Filtered · 2h 00m of 3h 00m/);
  await page.keyboard.press("w");
  await expect(page).toHaveURL(/\/timeline\/week.*project=/);
  await page.goBack();
  await expect(page).toHaveURL(/\/timeline\/day.*project=/);
  await page.keyboard.press("Shift+F");
  await expect(page).not.toHaveURL(/project=/);
  await expect(page.getByTestId("filter-chips")).toHaveCount(0);
  await expect(page.locator("[data-dimmed]")).toHaveCount(0);
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
  // Duration 15 min defaults the start grid to 15 min; the brief's example uses start off.
  await expect(page.getByRole("button", { name: "Start", exact: true })).toContainText("15 min");
  await page.getByRole("button", { name: "Start", exact: true }).click();
  await expect(page.getByRole("menuitemradio", { name: /^10 min/ })).toHaveAttribute("data-disabled", "");
  await page.getByRole("menuitemradio", { name: "off" }).click();
  const raw = await page.getByTestId("raw-row").evaluateAll((rows) => rows.map((r) => [Number(r.getAttribute("data-start")), Number(r.getAttribute("data-end"))]));
  const flat = await page.getByTestId("flat-row").evaluateAll((rows) => rows.map((r) => [Number(r.getAttribute("data-start")), Number(r.getAttribute("data-end"))]));
  expect(raw).toHaveLength(2);
  const expected = roundAndFlatten(
    raw.map(([start, end], i) => ({ task: "t", start: start!, end: end!, entries: [String(i)] })),
    { gridSeconds: 15 * 60, mode: "up", startGridSeconds: 0, startMode: "up", group: "entry" },
    "Europe/Berlin",
  ).map((r) => [r.start, r.end]);
  expect(flat).toEqual(expected);
  const texts = await page.getByTestId("flat-row").allInnerTexts();
  expect(texts[0]).toContain("13:00–13:15");
  expect(texts[1]).toContain("13:15–14:00");
});

/** `YYYY-MM-DD` of this week's Monday + `offset` days in Europe/Berlin. */
function weekDay(offset: number): string {
  const today = new Intl.DateTimeFormat("en-CA", { timeZone: "Europe/Berlin" }).format(new Date());
  const date = new Date(`${today}T12:00:00Z`);
  date.setUTCDate(date.getUTCDate() - ((date.getUTCDay() + 6) % 7) + offset);
  return date.toISOString().slice(0, 10);
}

test.describe("reports week", () => {
  test("filter via F, sticky day header, rounded CSV respects the filter", async ({ page }) => {
    await login(page, user(22));
    await createTask(page, "Atlas job @Atlas");
    await createTask(page, "Billing job @Billing");
    await page.goto(`/timeline/day?date=${weekDay(0)}`);
    for (const h of [8, 9, 10, 11, 12, 13, 14, 15]) await createEntry(page, [h, 0], [h, 40], h % 2 ? "billing job" : "atlas job");
    await page.goto(`/timeline/day?date=${weekDay(1)}`);
    await createEntry(page, [9, 0], [10, 0], "atlas job");
    await createEntry(page, [11, 0], [12, 0], "billing job");

    await page.goto("/reports");
    await page.getByRole("radiogroup", { name: "Range" }).getByRole("radio", { name: "Week" }).click();
    await expect(page.getByTestId("report-totals")).toContainText("Entries 10");
    await page.keyboard.press("f");
    await expect(page.getByTestId("timeline-filter")).toBeVisible();
    await page.keyboard.type("atlas");
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/reports\?.*project=/);
    await expect(page.getByTestId("report-totals")).toContainText("Filtered ·");
    await expect(page.getByTestId("report-totals")).toContainText("Entries 5 of 10");
    await expect(page.getByTestId("day-header")).toHaveCount(2);
    // A short window so the preview scrolls.
    await page.setViewportSize({ width: 1440, height: 520 });

    // Scroll past Monday's rows: Tuesday's header pins at the top and drives the day strip.
    const scroller = page.getByTestId("preview-scroll");
    await scroller.evaluate((el) => {
      const tue = el.querySelectorAll<HTMLElement>("[data-day]")[1]!;
      el.scrollTop = tue.offsetTop + 10;
    });
    const tueHeader = page.getByTestId("day-header").nth(1);
    await expect(page.getByTestId("strip-day")).toContainText("Tue");
    const top = (await scroller.boundingBox())!.y;
    expect(Math.abs((await tueHeader.boundingBox())!.y - top)).toBeLessThanOrEqual(2);

    const download = page.waitForEvent("download");
    await page.getByTestId("download").click();
    const file = await (await download).path();
    const csv = (await import("node:fs")).readFileSync(file, "utf8");
    expect(csv).toContain("Atlas job");
    expect(csv).not.toContain("Billing job");
  });
});

test("timeline grid chip: session override with accent dot, Reset to default", async ({ page }) => {
  await login(page, user(23));
  const chip = page.getByTestId("grid-menu");
  await expect(chip).toContainText("Start 15m · Dur 15m");
  await expect(page.getByTestId("grid-overridden")).toHaveCount(0);
  await chip.click();
  await page.getByRole("menuitemradio", { name: "5 min" }).first().click();
  await expect(chip).toContainText("Start 5m · Dur 15m");
  await expect(page.getByTestId("grid-overridden")).toHaveCount(1);
  // ⌥1 no longer changes the grids.
  await page.keyboard.press("Alt+1");
  await expect(chip).toContainText("Start 5m · Dur 15m");
  await chip.click();
  await page.getByRole("menuitem", { name: /Reset to default/ }).click();
  await expect(chip).toContainText("Start 15m · Dur 15m");
  await expect(page.getByTestId("grid-overridden")).toHaveCount(0);
});

test.describe("reports on phone", () => {
  test("rows stack the raw time above the rounded time", async ({ browser }) => {
    const desktop = await browser.newPage({ viewport: { width: 1440, height: 900 }, timezoneId: "Europe/Berlin" });
    await login(desktop, user(24));
    await createTask(desktop, "Phone job");
    await createEntry(desktop, [9, 7], [9, 52], "phone job", true);
    await desktop.close();
    const phone = await browser.newPage({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true, timezoneId: "Europe/Berlin" });
    await login(phone, user(24));
    await phone.goto("/reports");
    await phone.getByRole("radiogroup", { name: "Range" }).getByRole("radio", { name: "Day" }).click();
    const row = phone.getByTestId("phone-row").first();
    await expect(row).toBeVisible();
    await expect(row.getByTestId("phone-raw")).toContainText(/09:0\d–09:5\d/);
    const raw = (await row.getByTestId("phone-raw").boundingBox())!;
    const rounded = (await row.getByTestId("phone-flat").boundingBox())!;
    expect(raw.y).toBeLessThan(rounded.y);
    await phone.close();
  });
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

test("delete a task: unused at once, with entries after confirming; undo restores both", async ({ page }) => {
  await login(page, user(11));
  await expect(page.getByRole("button", { name: /New task/ })).toBeVisible();
  await createTask(page, "Unused scratch");
  await expect(page.getByTestId("side-task")).toHaveCount(1);
  await createTask(page, "Billable work");
  await expect(page.getByTestId("side-task")).toHaveCount(2);
  await page.getByRole("button", { name: "Start Billable work" }).click(); // one running entry
  await expect(page.getByTestId("running-chip")).toHaveCount(1);

  // Unused task: no dialog, toast, back on the list, undo brings it back.
  await page.keyboard.press("2");
  await page.getByTestId("task-row").filter({ hasText: "Unused scratch" }).getByRole("button", { name: "Unused scratch", exact: true }).click();
  await page.getByTestId("task-delete").click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page).toHaveURL(/\/tasks$/);
  await expect(page.locator("[data-sonner-toast]").first()).toContainText("Deleted #1 Unused scratch");
  await expect(page.getByTestId("task-row").filter({ hasText: "Unused scratch" })).toHaveCount(0);
  await page.keyboard.press("Control+z");
  await expect(page.getByTestId("task-row").filter({ hasText: "Unused scratch" })).toHaveCount(1);

  // Task with an entry: confirm first.
  await page.getByTestId("task-row").filter({ hasText: "Billable work" }).getByRole("button", { name: "Billable work", exact: true }).click();
  await page.getByTestId("task-delete").click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("This task has 1 time entry");
  await dialog.getByRole("button", { name: "Delete task and 1 entry" }).click();
  await expect(page).toHaveURL(/\/tasks$/);
  await expect(page.getByTestId("task-row").filter({ hasText: "Billable work" })).toHaveCount(0);
  await expect(page.getByTestId("running-chip")).toHaveCount(0);

  // One Ctrl+Z restores the task and its entry.
  await page.keyboard.press("Control+z");
  await expect(page.getByTestId("running-chip")).toHaveCount(1);
  await expect(page.getByTestId("task-row").filter({ hasText: "Billable work" })).toHaveCount(1);
});

// Spec: one Escape closes the picker and drops the pending range. This case passes on the
// code as of web-polish (the two-press report did not reproduce here); kept as a regression guard.
test("Escape closes the task picker in one press and drops the pending range", async ({ page }) => {
  await login(page, user(16));
  await createTask(page, "Login flow +auth");
  const column = page.getByTestId("day-column").first();
  await drag(page, await at(column, 13, 7), await at(column, 13, 52));
  await expect(page.getByTestId("task-picker")).toBeVisible();
  await page.getByPlaceholder("Search tasks, tags, ticket ids…").press("l");
  await page.keyboard.type("og");
  await expect(page.getByRole("option", { name: /Login flow/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("task-picker")).toHaveCount(0);
  await expect(page.getByTestId("create-ghost")).toHaveCount(0);
});

test("Timeline and Tasks reopen on the last range and mode", async ({ page }) => {
  await login(page, user(16));
  await page.getByRole("radio", { name: /Week/ }).click();
  await expect(page).toHaveURL(/\/timeline\/week/);
  await page.getByRole("link", { name: /Tasks/ }).first().click();
  await page.keyboard.press("1");
  await expect(page).toHaveURL(/\/timeline\/week/);

  await page.keyboard.press("2");
  await page.getByRole("radio", { name: /Board/ }).click();
  await expect(page).toHaveURL(/view=board/);
  await page.getByRole("link", { name: /Reports/ }).first().click();
  await page.keyboard.press("2");
  await expect(page).toHaveURL(/\/tasks\?view=board/);
});

test("timeline Select mode: marquee, ⌫ deletes with one toast, one Ctrl+Z restores all", async ({ page }) => {
  await login(page, user(18));
  await expect(page.getByRole("button", { name: /New task/ })).toBeVisible();
  await createTask(page, "Bulk entries");
  await expect(page.getByTestId("side-task")).toHaveCount(1);
  await createEntry(page, [9, 0], [10, 0], "Bulk");
  await createEntry(page, [11, 0], [12, 0], "Bulk");
  await createEntry(page, [13, 0], [14, 0], "Bulk");
  const blocks = page.getByTestId("entry-block");
  await expect(blocks).toHaveCount(3);

  await page.keyboard.press("v");
  const column = page.getByTestId("day-column").first();
  await drag(page, await at(column, 8, 30), await at(column, 14, 30));
  await expect(page.getByTestId("selection-bar")).toContainText("3 entries");
  await expect(page.getByTestId("create-ghost")).toHaveCount(0);

  await page.keyboard.press("Backspace");
  await expect(blocks).toHaveCount(0);
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Deleted 3 entries" })).toHaveCount(1);

  await page.keyboard.press("Control+z");
  await expect(blocks).toHaveCount(3);
});

test("tasks: shift-click a range with entries, confirm once, one Ctrl+Z restores tasks and entries", async ({ page }) => {
  await login(page, user(19));
  await expect(page.getByRole("button", { name: /New task/ })).toBeVisible();
  for (const name of ["Range one", "Range two", "Range three"]) {
    await createTask(page, name, true);
    await expect(page.getByTestId("side-task")).not.toHaveCount(0);
  }
  await page.keyboard.press("2");
  const rows = page.getByTestId("task-row");
  await expect(rows).toHaveCount(3);
  await rows.nth(0).getByRole("checkbox").click();
  await rows.nth(2).getByRole("checkbox").click({ modifiers: ["Shift"] });
  await expect(page.getByTestId("task-selection-bar")).toContainText("3 tasks");

  await page.keyboard.press("Backspace");
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("Delete 3 tasks and 3 entries?");
  await dialog.getByRole("button", { name: "Delete 3 tasks and 3 entries" }).click();
  await expect(rows).toHaveCount(0);
  await expect(page.locator("[data-sonner-toast]").first()).toContainText("Deleted 3 tasks and 3 entries");

  await page.keyboard.press("Control+z");
  await expect(rows).toHaveCount(3);
  await expect(page.getByTestId("running-chip")).toHaveCount(3);
});
