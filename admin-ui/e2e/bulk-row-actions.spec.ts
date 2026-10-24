import { expect, signIn, test } from "./fixtures";
import { createPost, trashPost } from "./records";

// `.bulk()` means also for selections: a bulk action is a row action too.
test("bulk actions are offered on each row too", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  const key = await createPost(page, "Bulk on a row", false);
  await page.goto("/admin/r/posts?search=Bulk+on+a+row");
  const row = page.getByRole("row").filter({ has: page.getByRole("cell", { name: "Bulk on a row", exact: true }) });
  await row.getByRole("button", { name: "More actions for Bulk on a row" }).click();
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem", { name: "Publish" })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Move to category…" })).toBeVisible();
  await menu.getByRole("menuitem", { name: "Publish" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Publish: 1 record done.")).toBeVisible();
  await trashPost(page, key);
});
