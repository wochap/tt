// Two tt-servers sharing a root: the app signed in on one keeps syncing
// through the other when the first goes away, without signing in again.
// The servers are this test's own (e2e/serve.sh builds the binary and the
// bundle); the shared e2e server is not used.

import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test } from "@playwright/test";

import { createTask, login, PASSWORD } from "./helpers.ts";

const SERVER = process.env.TT_SERVER_BIN ?? fileURLToPath(new URL("../../target/debug/tt-server", import.meta.url));
const WEB = fileURLToPath(new URL("../dist", import.meta.url));

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as { port: number };
      server.close(() => resolve(port));
    });
  });
}

function admin(db: string, args: string[], input?: string): string {
  const result = spawnSync(SERVER, ["--db", db, ...args], { input, encoding: "utf8", timeout: 60_000 });
  if (result.status !== 0) throw new Error(`tt-server ${args.join(" ")}: ${result.stderr}`);
  return result.stdout;
}

interface Member {
  url: string;
  db: string;
  process: ChildProcess;
  dir: string;
}

/** A member serving the bundle with its client URL; `name` creates its root first. */
async function member(name: string | null): Promise<Member> {
  const dir = mkdtempSync(join(tmpdir(), "tt-web-failover-"));
  const db = join(dir, "server.db");
  if (name) admin(db, ["init", "--name", name]);
  const [http, peer] = [await freePort(), await freePort()];
  const url = `http://127.0.0.1:${http}`;
  const args = ["--db", db, "serve", "--insecure-http", "--listen", `127.0.0.1:${http}`, "--web-dir", WEB];
  args.push("--peer-listen", `127.0.0.1:${peer}`, "--public-url", url);
  const process = spawn(SERVER, args, { stdio: "ignore" });
  await expect
    .poll(() => fetch(`${url}/api/health`).then((response) => response.ok, () => false), { timeout: 20_000 })
    .toBe(true);
  return { url, db, process, dir };
}

function stop(member: Member): Promise<void> {
  if (member.process.exitCode !== null) return Promise.resolve();
  return new Promise((resolve) => {
    member.process.once("exit", () => resolve());
    member.process.kill();
  });
}

test("the app syncs through another member when the one it signed in to stops", async ({ page }) => {
  const a = await member("laptop-a");
  const b = await member(null);
  try {
    admin(a.db, ["user", "add", "e2e-failover"], `${PASSWORD}\n`);
    const code = admin(a.db, ["peer", "invite"]).trim();
    admin(b.db, ["peer", "join", code, "--name", "laptop-b"]);
    // Both members know each other's client URL once linked.
    await expect.poll(() => admin(a.db, ["peer", "ls"]), { timeout: 20_000 }).toMatch(/laptop-b \(\w+\)\s+(online|syncing)/);

    await login(page, "e2e-failover", a.url);
    await expect(page.getByTestId("sync-status")).toContainText("laptop-a");
    await createTask(page, "Before failover");
    await expect(page.getByTestId("sync-status")).toHaveAttribute("data-state", "synced");
    // The worker learned B from A's `/api/peers` and the tab stored it with the session.
    await expect
      .poll(() => page.evaluate(() => (JSON.parse(localStorage.getItem("tt.auth") ?? "{}").endpoints ?? []).length), { timeout: 20_000 })
      .toBe(2);
    await page.evaluate(() => navigator.serviceWorker.ready);
    const token = await page.evaluate(() => JSON.parse(localStorage.getItem("tt.auth")!).token as string);

    // A goes away; the app (its shell from the service worker cache) moves to B.
    await stop(a);
    await page.reload();
    await expect(page.getByTestId("side-task")).toContainText("Before failover", { timeout: 20_000 });
    await expect(page.getByTestId("sync-status")).toContainText("laptop-b", { timeout: 20_000 });
    await expect(page.getByTestId("sync-status")).toHaveAttribute("data-state", /synced|syncing/);
    await expect(page.getByRole("button", { name: /Sign in/ })).toHaveCount(0);

    // A change made now reaches B.
    await createTask(page, "After failover");
    await expect(page.getByTestId("sync-status")).toHaveAttribute("data-state", "synced", { timeout: 20_000 });
    await expect
      .poll(
        async () => {
          const response = await fetch(`${b.url}/api/export`, { headers: { authorization: `Bearer ${token}` } });
          return response.ok ? JSON.stringify(await response.json()).includes("After failover") : false;
        },
        { timeout: 20_000 },
      )
      .toBe(true);
  } finally {
    await Promise.all([stop(a), stop(b)]);
    rmSync(a.dir, { recursive: true, force: true });
    rmSync(b.dir, { recursive: true, force: true });
  }
});
