import { expect, signIn, srgb, test } from "./fixtures";

// Runtime branding (ADR 0033): the demo's branding source shows the site's
// name and a primary color, on the shell and on the sign-in page.
test("the site's name brands the panel and the sign-in page", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/settings/site");
  await page.getByLabel(/^Site name/).fill("Brandville");
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(page.getByText("Settings saved.")).toBeVisible();
  // Cached for a second (`[admin] branding_ttl_secs`); each reload asks again.
  await expect(async () => {
    await page.reload();
    await expect(page.getByRole("link", { name: "Brandville" }).first()).toBeVisible({ timeout: 1000 });
  }).toPass({ timeout: 10_000 });
  await expect(page).toHaveTitle(/^Brandville · /);
  const primary = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--primary").trim());
  expect(primary).toBe("#7c3aed");
  // Buttons, and their hover (color-mix in the panel's stylesheet), follow the application's primary.
  const button = await page.locator("button.bg-primary").first().evaluate((element) => getComputedStyle(element).backgroundColor);
  const [hover = [], buttonColor] = await srgb(page, ["--primary-hover", button]);
  expect(buttonColor).toEqual([0x7c, 0x3a, 0xed]);
  // Violet, darker than the primary (away from the white text).
  expect(hover[2]!).toBeGreaterThan(hover[1]! + 60);
  expect(hover[2]!).toBeLessThan(0xed);

  await page.context().clearCookies();
  await page.goto("/admin/login");
  await expect(page.getByText("Brandville", { exact: true })).toBeVisible();
  await expect(page).toHaveTitle("Brandville");
});
