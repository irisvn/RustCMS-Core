import { expect, signIn, test } from "./fixtures";

// The demo's React page (host API v2, built with the SDK's preset): it renders
// in the panel's tree, reads an AdminRoutes GET, streams, posts with CSRF,
// opens sub-pages and a dialog styled by its own CSS, under the real CSP, on
// one React.
test("a v2 React page shares the panel and its sub-pages work", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/p/react-demo");
  await expect(page.getByText("Hello from AdminRoutes")).toBeVisible();
  // The counter streams, and the reset is a POST with the CSRF token.
  const count = page.getByLabel("React count");
  await expect.poll(async () => Number(await count.textContent()), { timeout: 10_000 }).toBeGreaterThanOrEqual(1);
  const reset = page.waitForRequest((r) => r.method() === "POST" && r.url().endsWith("/admin/api/x/demo/ticks/reset"));
  await page.getByRole("button", { name: "Reset counter" }).click();
  expect((await reset).headers()["x-csrf-token"]).toBeTruthy();
  // A sub-page: the server's `data` knows it; it survives a reload and Back.
  await page.getByRole("button", { name: "Second" }).click();
  await expect(page).toHaveURL(/\/admin\/p\/react-demo\/items\/2$/);
  await expect(page.getByRole("heading", { name: "Item Second" })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("heading", { name: "Item Second" })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole("heading", { name: "Item Second" })).toBeHidden();
  // A dialog (a portal), styled by the extension's own stylesheet.
  await page.getByRole("button", { name: "Open dialog" }).click();
  const styled = page.getByText("Styled by the extension");
  await expect(styled).toBeVisible();
  await expect(styled).toHaveCSS("font-style", "italic");
  await expect(page.getByText("You may watch the live counter.")).toBeVisible();
  // One React: the extension uses the panel's own.
  const reactCopies = await page.evaluate(() => {
    const scripts = [...document.scripts].map((s) => s.src);
    return scripts.filter((s) => /react(-dom)?(\.production)?(\.min)?\.js/.test(s)).length;
  });
  expect(reactCopies).toBe(0);
});
