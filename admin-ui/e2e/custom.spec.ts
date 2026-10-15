import { expect, signIn, test } from "./fixtures";

test("a custom page and a slot render from server templates, within the CSP", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Post stats" }).click();
  await expect(page.getByRole("heading", { name: "Post stats" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Latest posts" })).toBeVisible();
  await expect(page.locator("meter.rc-meter")).toBeVisible();
  // A link of the template navigates inside the panel, to a post with its slot.
  await page.locator(".custom-html a").first().click();
  await expect(page).toHaveURL(/\/admin\/r\/posts\/records\//);
  await expect(page.getByRole("heading", { name: "Visibility" })).toBeVisible();
  await expect(page.getByText(/Visible to readers\.|A draft: readers do not see it\./)).toBeVisible();
});
