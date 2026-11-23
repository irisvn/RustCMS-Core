import type { Page } from "@playwright/test";
import { expect, signIn, test } from "./fixtures";

// Repeaters, second round (ADR 0044, part A), on the demo's categories: the
// posts repeater is a table, shown while the category lists its posts; the
// view page shows the posts under the category; making the category a link
// hides them and keeps them (shown again, they are there). Nothing is left behind.

/** Deletes a trashed post for good. */
async function purge(page: Page, title: string) {
  await page.goto(`/admin/r/posts?search=${encodeURIComponent(title)}`);
  await page.getByLabel("Trashed records").selectOption("only");
  await page.getByRole("row", { name: title }).getByRole("button", { name: "Delete permanently" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Deleted permanently.")).toBeVisible();
}

test("a category's posts follow its layout: a table, its view page, kept while it is a link", async ({ page, watch: _ }) => {
  const run = Date.now().toString(36);
  const name = `E2E layout ${run}`;
  const [first, second] = [`Layout one ${run}`, `Layout two ${run}`];
  await signIn(page, "admin@example.test");

  // A table of posts: the header names the columns, each cell keeps its label.
  await page.goto("/admin/r/categories/create");
  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Slug").fill(`e2e-layout-${run}`);
  await page.getByRole("button", { name: "Add a post" }).click();
  await page.getByRole("button", { name: "Add a post" }).click();
  const rows = page.getByRole("main").getByRole("listitem");
  await expect(rows).toHaveCount(2);
  await rows.nth(0).getByLabel(/^Title/).fill(first);
  await rows.nth(1).getByLabel(/^Title/).fill(second);
  // One table row: the title and the switch side by side.
  const title = await rows.nth(0).getByLabel(/^Title/).boundingBox();
  const published = await rows.nth(0).getByRole("switch", { name: /^Published/ }).boundingBox();
  expect(Math.abs(title!.y + title!.height / 2 - (published!.y + published!.height / 2))).toBeLessThan(24);
  expect(published!.x).toBeGreaterThan(title!.x + title!.width);
  // Not a link: no link to fill.
  await expect(page.getByLabel(/^Link/)).toHaveCount(0);
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name })).toBeVisible();
  const url = page.url();

  // The view page lists the posts under the category, as the relation's section does.
  const main = page.getByRole("main");
  await expect(main.getByRole("listitem").filter({ hasText: first })).toBeVisible();
  await expect(main.getByRole("listitem").filter({ hasText: second })).toBeVisible();

  // Made a link: the posts are hidden, not removed (no notice).
  await page.goto(`${url}/edit`);
  await expect(rows).toHaveCount(2);
  await page.getByLabel(/^Layout/).selectOption({ label: "A link" });
  await expect(rows).toHaveCount(0);
  await expect(page.getByRole("status").filter({ hasText: /removes/ })).toHaveCount(0);
  await page.getByLabel(/^Link/).fill("https://example.test/elsewhere");
  // Back and forth keeps the drafts: shown again, the rows are there.
  await page.getByLabel(/^Layout/).selectOption({ label: "Its posts" });
  await expect(rows).toHaveCount(2);
  await page.getByLabel(/^Layout/).selectOption({ label: "A link" });
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page).toHaveURL(url);
  // The view page follows the layout (no posts listed under it); the relation's section still has them.
  await expect(main.getByRole("listitem").filter({ hasText: first })).toHaveCount(0);
  await expect(page.getByRole("table", { name: "Posts" }).getByText(first)).toBeVisible();
  await expect(main.getByRole("link", { name: "https://example.test/elsewhere" })).toBeVisible();

  // Its posts again: they were kept. Removed here (to the trash), then left nowhere.
  await page.goto(`${url}/edit`);
  await page.getByLabel(/^Layout/).selectOption({ label: "Its posts" });
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0).getByLabel(/^Title/)).toHaveValue(first);
  await rows.nth(1).getByRole("button", { name: "Remove" }).click();
  await rows.nth(0).getByRole("button", { name: "Remove" }).click();
  await expect(rows).toHaveCount(0);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page).toHaveURL(url);

  // Leave nothing behind.
  await purge(page, first);
  await purge(page, second);
  await page.goto(`/admin/r/categories?search=${encodeURIComponent(name)}`);
  await page.getByRole("row", { name }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByRole("row", { name })).toHaveCount(0);
});
