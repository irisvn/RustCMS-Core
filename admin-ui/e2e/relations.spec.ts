import { expect, signIn, test } from "./fixtures";

// A category's color comes from the demo's extension widget, and its posts are
// listed and created on its page. The CSP watcher of `fixtures.ts` covers both
// the extension script and its stylesheet.
test("a custom widget and a has-many relation on the category page", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/categories/create");
  await page.getByLabel("Name").fill("E2E colors");
  await page.getByLabel("Slug").fill("e2e-colors");
  const color = page.getByRole("group", { name: "Color" });
  await color.getByRole("button", { name: "Blue" }).click();
  await expect(color.getByRole("button", { name: "Blue" })).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: "E2E colors" })).toBeVisible();
  await expect(page.getByRole("main").getByText("#2563eb")).toBeVisible();
  const url = page.url();

  // Editing: the widget gets the stored value and can clear it.
  await page.goto(`${url}/edit`);
  const edit = page.getByRole("group", { name: "Color" });
  await expect(edit.getByRole("button", { name: "Blue" })).toHaveAttribute("aria-pressed", "true");
  await edit.getByRole("button", { name: "None" }).click();
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("heading", { name: "E2E colors" })).toBeVisible();
  await expect(page.getByRole("main").getByText("#2563eb")).toHaveCount(0);

  // The relation: empty, then a post created from here belongs to this category.
  await page.goto(url);
  const posts = page.getByRole("table", { name: "Posts" });
  await expect(posts).toBeVisible();
  await page.getByRole("link", { name: /New/ }).click();
  await page.getByLabel("Title").fill("Related post");
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page).toHaveURL(url);
  await expect(posts.getByText("Related post")).toBeVisible();

  // Leave nothing behind: the post (trash, then for good), then the category.
  await page.goto("/admin/r/posts?search=Related+post");
  await page.getByRole("row", { name: /Related post/ }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await page.getByLabel("Trashed records").selectOption("only");
  await page.getByRole("row", { name: /Related post/ }).getByRole("button", { name: "Delete permanently" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Deleted permanently.")).toBeVisible();
  await page.goto("/admin/r/categories?search=E2E+colors");
  await page.getByRole("row", { name: /E2E colors/ }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByRole("row", { name: /E2E colors/ })).toHaveCount(0);
});
