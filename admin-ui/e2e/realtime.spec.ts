import { expect, signIn, test } from "./fixtures";

// The demo's live page (`LivePage`): server-sent events from an `AdminRoutes`
// stream, admitted on the browser's own fetch metadata (no CSRF token, no
// Origin on a same-origin GET), under the real CSP, reset by a POST, and
// stopped when the user signs out.
test("a live page streams, resets and stops on sign-out", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  const stream = page.waitForResponse((r) => r.url().endsWith("/admin/api/x/demo/ticks"));
  await page.goto("/admin/p/live");
  expect((await stream).status()).toBe(200);
  const count = page.getByLabel("Count");
  await expect.poll(async () => Number(await count.textContent()), { timeout: 10_000 }).toBeGreaterThanOrEqual(2);

  await page.getByRole("button", { name: "Reset" }).click();
  await expect.poll(async () => Number(await count.textContent()), { timeout: 5_000 }).toBeLessThanOrEqual(1);

  // Signing out ends the view and its stream: no request for it afterwards.
  let after = 0;
  page.on("request", (request) => {
    if (request.url().endsWith("/admin/api/x/demo/ticks")) after += 1;
  });
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await page.waitForTimeout(3_000);
  expect(after).toBe(0);
});

// The same page's echo socket (`AdminRoutes::socket`): a same-origin
// WebSocket carrying the CSRF token as a subprotocol, under the real CSP; a
// command (`whoami`) is authorized when it runs.
test("a live page talks over a WebSocket", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  const opened = page.waitForEvent("websocket", (ws) => ws.url().endsWith("/admin/api/x/demo/echo"));
  await page.goto("/admin/p/live");
  const socket = await opened;
  const message = page.getByLabel("Message");
  const echo = page.getByLabel("Echo");
  await expect.poll(async () => {
    await message.fill("hello");
    await page.getByRole("button", { name: "Send" }).click();
    return echo.textContent();
  }, { timeout: 10_000 }).toBe("hello");
  await message.fill("whoami");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(echo).toHaveText(/^admin:\S+$/);
  // Leaving the page closes it.
  const closed = new Promise<void>((resolve) => socket.on("close", () => resolve()));
  await page.goto("/admin");
  await closed;
});
