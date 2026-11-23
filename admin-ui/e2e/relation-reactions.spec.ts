import { expect, signIn, test } from "./fixtures";

// A reaction on relationship rows (ADR 0044, part D): the demo's categories
// title a new post switched to published without a title ("Untitled post
// N"); the user may change it, and the save writes what the form holds.
// Everything is removed again at the end.

test("a reaction fills a new row of a category's posts", async ({ page, watch: _ }) => {
  const run = Date.now().toString(36);
  const name = `E2E row reaction ${run}`;
  const title = `Reacted ${run}`;
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/categories/create");
  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Slug").fill(`e2e-row-reaction-${run}`);
  const rows = page.getByRole("main").getByRole("listitem");
  await page.getByRole("button", { name: "Add a post" }).click();
  await expect(rows).toHaveCount(1);
  // Published without a title: the server's reaction fills it in.
  await rows.nth(0).getByRole("switch", { name: /^Published/ }).click();
  await expect(rows.nth(0).getByLabel(/^Title/)).toHaveValue("Untitled post 1");
  // The user's own title stays.
  await rows.nth(0).getByLabel(/^Title/).fill(title);
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name })).toBeVisible();
  const url = page.url();
  await expect(page.getByRole("table", { name: "Posts" }).getByText(title)).toBeVisible();

  // Leave nothing behind: the post (removed, then deleted for good) and the category.
  await page.goto(`${url}/edit`);
  await rows.nth(0).getByRole("button", { name: "Remove" }).click();
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page).toHaveURL(url);
  await page.goto(`/admin/r/posts?search=${encodeURIComponent(title)}`);
  await page.getByLabel("Trashed records").selectOption("only");
  await page.getByRole("row", { name: title }).getByRole("button", { name: "Delete permanently" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Deleted permanently.")).toBeVisible();
  await page.goto(`/admin/r/categories?search=${encodeURIComponent(name)}`);
  await page.getByRole("row", { name }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByRole("row", { name })).toHaveCount(0);
});
