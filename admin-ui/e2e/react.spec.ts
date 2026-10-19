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

// Level 2: the resource API, toasts, tabs on a sub-path, a menu and a
// tooltip, all under the production CSP (no inline style: positions are
// per-property CSSOM writes).
test("a ui 2 React page writes through the resource API with toasts, menus and tooltips", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/p/react-demo");
  await page.getByRole("tab", { name: "Tags" }).click();
  await expect(page).toHaveURL(/\/admin\/p\/react-demo\/tags$/);
  await expect(page.getByRole("tab", { name: "Tags" })).toHaveAttribute("aria-selected", "true");
  // `props.title`: the heading and the breadcrumb name the sub-path.
  await expect(page.getByRole("heading", { level: 1, name: "React demo: tags" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("React demo: tags");
  // A refused create shows the resource's own validation message.
  await page.getByRole("button", { name: "Add tag" }).click();
  await expect(page.locator(".demo-react-error")).toBeVisible();
  // A create with an idempotency key, then a toast.
  const name = `e2e-${Date.now()}`;
  const created = page.waitForRequest((r) => r.method() === "POST" && r.url().endsWith("/admin/api/resources/tags/records"));
  await page.getByLabel("Tag name").fill(name);
  await page.getByRole("button", { name: "Add tag" }).click();
  expect((await created).headers()["idempotency-key"]).toBeTruthy();
  await expect(page.getByRole("status").getByText("Tag created")).toBeVisible();
  await expect(page.locator(".demo-react-list").getByText(name)).toBeVisible();
  // A tooltip, placed by the button and on top.
  const refresh = page.getByRole("button", { name: "Refresh" });
  await refresh.hover();
  const tip = page.getByRole("tooltip");
  await expect(tip).toHaveText("Read the tags again");
  const [anchor, box] = [await refresh.boundingBox(), await tip.boundingBox()];
  expect(Math.abs(box!.y - (anchor!.y + anchor!.height)) < 12 || Math.abs(box!.y + box!.height - anchor!.y) < 12).toBe(true);
  const onTop = await page.evaluate(([x, y]) => document.elementFromPoint(x!, y!)?.closest('[role="tooltip"]') !== null, [box!.x + box!.width / 2, box!.y + box!.height / 2]);
  expect(onTop).toBe(true);
  // The menu: keyboard, destructive item, then a toast.
  await page.getByRole("button", { name: `Actions ${name}` }).click();
  await expect(page.getByRole("menuitem", { name: "Open" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  const deleted = page.waitForRequest((r) => r.method() === "DELETE" && r.url().includes("/admin/api/resources/tags/records/"));
  await page.keyboard.press("Enter");
  await deleted;
  await expect(page.getByRole("status").getByText("Tag deleted")).toBeVisible();
  await expect(page.locator(".demo-react-list").getByText(name)).toBeHidden();
});
