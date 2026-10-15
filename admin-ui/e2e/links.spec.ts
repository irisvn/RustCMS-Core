import { expect, signIn, test } from "./fixtures";

// A post's tags (belongs-to-many, with a `position` pivot column), an action
// with a form (`Move to category…`) and the role permission matrix. Leaves
// nothing behind.
test("tags are attached, edited and detached; an action asks for a category", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts/create");
  await page.getByLabel("Title").fill("Tagged post");
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: "Tagged post" })).toBeVisible();
  const url = page.url();
  const tags = page.getByRole("table", { name: "Tags" });
  await expect(tags).toBeVisible();

  // Attach two tags with a position.
  await page.getByRole("button", { name: "Attach" }).click();
  const attach = page.getByRole("dialog", { name: "Attach to Tags" });
  await attach.getByRole("checkbox", { name: "Rust" }).check();
  await attach.getByRole("checkbox", { name: "Design" }).check();
  await attach.getByLabel("Position").fill("1");
  await attach.getByRole("button", { name: "Attach" }).click();
  await expect(page.getByText("2 attached.")).toBeVisible();
  await expect(tags.getByRole("row", { name: /Rust/ })).toContainText("1");

  // Edit one link, then detach the other.
  await tags.getByRole("button", { name: "Edit link: Rust" }).click();
  const edit = page.getByRole("dialog", { name: "Edit link" });
  await edit.getByLabel("Position").fill("4");
  await edit.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByText("Link updated.")).toBeVisible();
  await expect(tags.getByRole("row", { name: /Rust/ })).toContainText("4");
  await tags.getByRole("checkbox", { name: "Select Design" }).check();
  await page.getByRole("button", { name: "Detach selected" }).click();
  await page.getByRole("dialog", { name: "Detach" }).getByRole("button", { name: "Detach" }).click();
  await expect(page.getByText("1 detached.")).toBeVisible();
  await expect(tags.getByRole("row", { name: /Design/ })).toHaveCount(0);

  // An action with a form: the category is chosen in a dialog.
  await page.goto("/admin/r/posts?search=Tagged+post");
  await page.getByRole("row", { name: /Tagged post/ }).getByRole("checkbox").check();
  await page.getByRole("button", { name: "Move to category…" }).click();
  const move = page.getByRole("dialog", { name: "Move to category…" });
  await move.getByRole("combobox", { name: /Category/ }).click();
  await page.getByRole("option", { name: "Guides" }).click();
  await move.getByRole("button", { name: "Run" }).click();
  await expect(move).toBeHidden();
  await expect(page.getByRole("row", { name: /Tagged post/ })).toContainText("Guides");

  // Leave nothing behind: trash, then delete for good (its links go with it).
  await page.getByRole("row", { name: /Tagged post/ }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await page.getByLabel("Trashed records").selectOption("only");
  await page.getByRole("row", { name: /Tagged post/ }).getByRole("button", { name: "Delete permanently" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Deleted permanently.")).toBeVisible();
  expect(url).toContain("/admin/r/posts/records/");
});

test("the role page lays resource permissions out as a matrix", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/system/roles/create");
  await expect(page.getByRole("checkbox", { name: "Posts: View" })).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "Posts: attach_tags" })).toBeVisible();
  await page.getByRole("checkbox", { name: "Create on every resource" }).check();
  await expect(page.getByRole("checkbox", { name: "Posts: Create" })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: "Tags: Create" })).toBeChecked();
  // A whole row: on, then off.
  const row = page.getByRole("checkbox", { name: "Every permission of Tags" });
  await row.check();
  await expect(page.getByRole("checkbox", { name: "Tags: Delete" })).toBeChecked();
  await row.uncheck();
  await expect(page.getByRole("checkbox", { name: "Tags: Create" })).not.toBeChecked();
});
