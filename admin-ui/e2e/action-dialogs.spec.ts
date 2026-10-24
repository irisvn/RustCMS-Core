import { expect, signIn, test } from "./fixtures";

// An action's dialog: titled without the menu label's ellipsis, run by its own button.
test("an action dialog is titled and run with the action's own words", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts?search=Post+number+5&sort=title");
  await page.getByLabel("Select Post number 5", { exact: true }).check();
  await page.getByRole("button", { name: "Move to category…" }).click();
  const dialog = page.getByRole("dialog", { name: "Move to category" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Move to category" })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Run" })).toHaveCount(0);
  await dialog.getByRole("button", { name: "Cancel" }).click();
});
