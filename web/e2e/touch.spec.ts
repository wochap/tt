import { expect, test } from "@playwright/test";

import { at, createTask, login, longPressDrag, touchGesture, user } from "./helpers.ts";

test.describe("phone touch", () => {
  test.use({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });

  test("long-press then drag creates a snapped range and opens the picker", async ({ page }) => {
    await login(page, user(12));
    await createTask(page, "Touch task");
    const column = page.getByTestId("day-column").first();
    await touchGesture(page, longPressDrag(await at(column, 13, 7), await at(column, 13, 52)));
    await expect(page.getByTestId("create-ghost")).toContainText("13:00 – 14:00");
    await expect(page.getByTestId("task-picker")).toBeVisible();
  });

  test("Select from the entry sheet, tap toggles, Done exits; long-press still moves an entry", async ({ page }) => {
    await login(page, user(20));
    await createTask(page, "Phone select");
    const column = page.getByTestId("day-column").first();
    for (const [from, to] of [[9, 10], [11, 12]] as const) {
      await touchGesture(page, longPressDrag(await at(column, from, 5), await at(column, to, 5)));
      await expect(page.getByTestId("task-picker")).toBeVisible();
      await page.keyboard.type("Phone");
      await page.keyboard.press("Enter");
      await expect(page.getByTestId("task-picker")).toBeHidden();
    }
    const blocks = page.getByTestId("entry-block");
    await expect(blocks).toHaveCount(2);

    // Tap opens the sheet; "Select" closes it and seeds the selection.
    await page.waitForTimeout(400); // clicks right after a drag end are swallowed
    await blocks.first().locator("[role=button]").first().click();
    await page.getByRole("button", { name: "Select", exact: true }).click();
    await expect(page.getByTestId("entry-sheet")).toBeHidden();
    const bar = page.getByTestId("phone-selection-bar");
    await expect(bar).toContainText("1 entry");

    // While selecting, a tap toggles instead of opening.
    await blocks.nth(1).locator("[role=button]").first().click();
    await expect(bar).toContainText("2 entries");
    await expect(page.getByTestId("entry-sheet")).toBeHidden();

    await bar.getByRole("button", { name: "Done" }).click();
    await expect(bar).toBeHidden();
    await blocks.first().locator("[role=button]").first().click();
    await expect(page.getByTestId("entry-sheet")).toBeVisible();
    await page.keyboard.press("Escape");

    // Long-press then drag still moves an entry and starts no selection.
    const before = (await blocks.first().boundingBox())!;
    await touchGesture(page, longPressDrag({ x: before.x + before.width / 2, y: before.y + before.height / 2 }, { x: before.x + before.width / 2, y: before.y + before.height / 2 + 60 }));
    await expect.poll(async () => (await blocks.first().boundingBox())!.y).toBeGreaterThan(before.y + 20);
    await expect(bar).toBeHidden();
  });

  test("quick swipe scrolls and creates nothing; a tap arms nothing", async ({ page }) => {
    await login(page, user(13));
    const scroller = page.getByTestId("timeline-scroll");
    const column = page.getByTestId("day-column").first();
    await scroller.evaluate((el) => (el.scrollTop = 0));
    const before = await scroller.evaluate((el) => el.scrollTop);
    const from = await at(column, 12, 0);
    await touchGesture(page, [
      { type: "start", at: from },
      ...[1, 2, 3, 4, 5, 6, 7, 8].map((i) => ({ type: "move" as const, at: { x: from.x, y: from.y - i * 25 }, wait: 16 })),
      { type: "end" },
    ]);
    await expect.poll(() => scroller.evaluate((el) => el.scrollTop)).toBeGreaterThan(before);
    await expect(page.getByTestId("create-ghost")).toHaveCount(0);
    await expect(page.getByTestId("task-picker")).toBeHidden();

    const tap = await at(column, 10, 0);
    await touchGesture(page, [{ type: "start", at: tap }, { type: "end", wait: 60 }]);
    await page.waitForTimeout(400);
    await expect(page.getByTestId("create-ghost")).toHaveCount(0);
    await expect(page.getByTestId("task-picker")).toBeHidden();
  });

  test("holding near the bottom edge scrolls and extends the range", async ({ page }) => {
    await login(page, user(14));
    const scroller = page.getByTestId("timeline-scroll");
    const column = page.getByTestId("day-column").first();
    await scroller.evaluate((el) => (el.scrollTop = 0));
    const start = await at(column, 9, 0);
    const box = (await scroller.boundingBox())!;
    const bottom = { x: start.x, y: box.y + box.height - 6 };
    const before = await scroller.evaluate((el) => el.scrollTop);
    const steps = longPressDrag(start, bottom);
    steps.splice(steps.length - 1, 0, { type: "move", at: bottom, wait: 900 });
    await touchGesture(page, steps);
    expect(await scroller.evaluate((el) => el.scrollTop)).toBeGreaterThan(before);
    await expect(page.getByTestId("task-picker")).toBeVisible();
    await expect(page.getByTestId("create-ghost")).not.toContainText("09:00 – 10:00");
  });
});

test.describe("phone task selection", () => {
  test.use({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });

  test("checkboxes and the action bar at 390 px", async ({ page }) => {
    await login(page, user(21));
    await page.goto("/tasks");
    const rows = page.locator("[data-task-row]");
    await createTask(page, "Phone task one");
    await expect(rows).toHaveCount(1);
    await createTask(page, "Phone task two");
    await expect(rows).toHaveCount(2);
    await rows.nth(0).getByRole("checkbox").check();
    const bar = page.getByTestId("phone-task-bar");
    await expect(bar).toContainText("1 task");
    await rows.nth(1).getByRole("checkbox").check();
    await expect(bar).toContainText("2 tasks");
    const box = (await bar.boundingBox())!;
    expect(box.y).toBeGreaterThan(650);
    await bar.getByRole("button", { name: "Delete" }).click();
    await expect(rows).toHaveCount(0);
    await bar.waitFor({ state: "detached" });
  });
});

test.describe("tablet touch board", () => {
  test.use({ hasTouch: true, isMobile: true, viewport: { width: 1000, height: 800 } });

  test("long-press then drag moves a card; a quick swipe moves nothing", async ({ page }) => {
    await login(page, user(15));
    await createTask(page, "Board card");
    await page.goto("/tasks?view=board");
    const card = page.getByTestId("board-card").first();
    await expect(card).toBeVisible();
    const from = (await card.boundingBox())!;
    const start = { x: from.x + from.width / 3, y: from.y + from.height / 2 };

    // Quick swipe over the card: no drag, state unchanged.
    await touchGesture(page, [
      { type: "start", at: start },
      ...[1, 2, 3, 4].map((i) => ({ type: "move" as const, at: { x: start.x + i * 40, y: start.y }, wait: 16 })),
      { type: "end" },
    ]);
    await expect(page.getByTestId("board-open").getByTestId("board-card")).toHaveCount(1);

    const done = (await page.getByTestId("board-done").boundingBox())!;
    await touchGesture(page, longPressDrag(start, { x: done.x + done.width / 2, y: done.y + 120 }));
    await expect(page.getByTestId("board-done").getByTestId("board-card")).toHaveCount(1);
  });
});
