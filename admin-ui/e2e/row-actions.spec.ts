import { expect, signIn, test } from "./fixtures";
import { createPost, trashPost } from "./records";

// Per-record actions: offered where the server says so, one labelled per row.
test("a row shows its one labelled action, decided per record by the server", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  const live = await createPost(page, "Row actions live", true);
  const draft = await createPost(page, "Row actions draft", false);
  await page.goto("/admin/r/posts?search=Row+actions&sort=title");
  const row = (title: string) => page.getByRole("row").filter({ has: page.getByRole("cell", { name: title, exact: true }) });
  // Published: Unpublish is its labelled action; a draft is not offered it.
  await expect(row("Row actions live").getByRole("button", { name: "Unpublish" })).toBeVisible();
  await expect(row("Row actions draft").getByRole("cell", { name: "Row actions draft", exact: true })).toBeVisible();
  await expect(row("Row actions draft").getByRole("button", { name: "Unpublish" })).toHaveCount(0);

  await row("Row actions live").getByRole("button", { name: "Unpublish" }).click();
  await expect(page.getByText("Unpublish: 1 record done.")).toBeVisible();
  await expect(row("Row actions live").getByRole("button", { name: "Unpublish" })).toHaveCount(0);
  await trashPost(page, live);
  await trashPost(page, draft);
});
