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
