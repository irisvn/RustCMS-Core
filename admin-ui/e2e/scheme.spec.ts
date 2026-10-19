import { expect, signIn, test } from "./fixtures";

// The user's color scheme: chosen in the sidebar, kept for this browser over
// the app's default, followed by React pages (`useColorScheme`).
test("the color scheme switch is kept and followed by React pages", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/p/react-demo");
  const html = page.locator("html");
  const group = page.getByRole("group", { name: "Color scheme" }).first();
  await group.getByRole("button", { name: "Dark" }).click();
  await expect(html).toHaveAttribute("data-scheme", "dark");
  await expect(page.getByLabel("React scheme")).toHaveText("dark");
  await page.reload();
  await expect(html).toHaveAttribute("data-scheme", "dark");
  await expect(group.getByRole("button", { name: "Dark" })).toHaveAttribute("aria-pressed", "true");
  await group.getByRole("button", { name: "Light" }).click();
  await expect(html).toHaveAttribute("data-scheme", "light");
  await expect(page.getByLabel("React scheme")).toHaveText("light");
});
