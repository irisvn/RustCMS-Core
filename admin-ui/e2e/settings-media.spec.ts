import { expect, signIn, test } from "./fixtures";

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

// A media setting: the logo of Settings → Site, picked with the media input.
test("a settings page keeps a file of the media library", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/settings/site");
  const logo = page.getByRole("group", { name: "Logo" });
  await expect(logo.getByText("No file.")).toBeVisible();
  await logo.locator("input[type=file]").setInputFiles({ name: "logo.png", mimeType: "image/png", buffer: PNG });
  await expect(logo.locator("img")).toBeVisible();
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(page.getByText("Settings saved.")).toBeVisible();
  await page.reload();
  await expect(page.getByRole("group", { name: "Logo" }).locator("img")).toBeVisible();
});
