import { expect, signIn, test } from "./fixtures";

// The dashboard's latest records show their values as the list does.
test("recent records on the dashboard show formatted values", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  const widget = page.getByRole("main").locator("div", { has: page.getByRole("heading", { name: "Latest posts" }) }).last();
  await expect(widget.getByRole("link").first()).toBeVisible();
  const line = (await widget.locator("li").first().textContent()) ?? "";
  expect(line).toContain("Published at:");
  expect(line).not.toMatch(/\d{4}-\d{2}-\d{2}T/);
});
