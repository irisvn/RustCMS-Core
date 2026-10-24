import { expect, signIn, test } from "./fixtures";
import { createPost, trashPost } from "./records";

// A record's page and the rows of relation tables show the actions they offer.
test("record pages and relation tables show the record's actions", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  const key = await createPost(page, "Record page actions", true);
  await page.goto(`/admin/r/posts/records/${key}`);
  await expect(page.getByRole("heading", { name: "Record page actions" })).toBeVisible();
  // Published: Unpublish labelled, the others under ⋯.
  await expect(page.getByRole("button", { name: "Unpublish" })).toBeVisible();
  await page.getByRole("button", { name: "More actions for Record page actions" }).click();
  await expect(page.getByRole("menuitem", { name: "Preview" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Unpublish" }).click();
  await expect(page.getByText("Unpublish: 1 record done.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Unpublish" })).toHaveCount(0);
  await trashPost(page, key);

  // A category's posts, each with its own actions.
  await page.goto("/admin/r/categories");
  await page.getByRole("row").nth(1).getByRole("link", { name: "View" }).click();
  const posts = page.getByRole("table", { name: "Posts" });
  await expect(posts.getByRole("button", { name: /^More actions for / }).first()).toBeVisible();
});
