import { expect, signIn, test } from "./fixtures";

test("an admin changes the site settings and sets a secret that is never shown again", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Site", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Site" })).toBeVisible();
  await page.getByLabel(/^Site name/).fill("Academy");
  await page.getByLabel("Posts per page").fill("0");
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(page.getByText("is out of range")).toBeVisible();
  await page.getByLabel("Posts per page").fill("20");
  await page.getByRole("button", { name: "Set a value" }).click();
  await page.getByLabel("Mail password").fill("mail-secret-value");
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(page.getByText("Settings saved.")).toBeVisible();
  await expect(page.getByText("Set", { exact: true })).toBeVisible();

  await page.reload();
  await expect(page.getByLabel(/^Site name/)).toHaveValue("Academy");
  await expect(page.getByLabel("Posts per page")).toHaveValue("20");
  await expect(page.locator("body")).not.toContainText("mail-secret-value");
});

test("editors have no settings pages", async ({ page, watch: _ }) => {
  await signIn(page, "editor@example.test");
  await expect(page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Site", exact: true })).toHaveCount(0);
  await page.goto("/admin/settings/site");
  await expect(page.getByRole("heading", { name: "Access denied" })).toBeVisible();
});
