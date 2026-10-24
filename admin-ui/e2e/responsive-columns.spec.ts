import { expect, signIn, test } from "./fixtures";

// `hide_below(Breakpoint::Md)`: the category column is dropped on narrow screens, by CSS only.
test("a column is hidden below its breakpoint", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/admin/r/posts");
  const header = page.getByRole("columnheader", { name: "Category" });
  await expect(header).toBeVisible();
  await page.setViewportSize({ width: 600, height: 900 });
  await expect(header).toBeHidden();
  await expect(page.getByRole("columnheader", { name: /^Title/ })).toBeVisible();
});
