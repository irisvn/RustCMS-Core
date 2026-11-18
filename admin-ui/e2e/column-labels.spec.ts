import { expect, signIn, test } from "./fixtures";

// A badge column without labels of its own shows the labels of the form's
// select (the demo's tag kinds), colored by value; nothing is written.
test("the tags list shows each kind by its label", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/tags?search=Security");
  const row = page.getByRole("row", { name: /Security/ });
  await expect(row.getByText("Series", { exact: true })).toBeVisible();
  await expect(row.getByText("series", { exact: true })).toHaveCount(0);
  await expect(row.getByText("Series", { exact: true })).toHaveClass(/text-primary/);

  await page.goto("/admin/r/tags?search=Rust");
  await expect(page.getByRole("row", { name: /Rust/ }).getByText("Topic", { exact: true })).toBeVisible();
});
