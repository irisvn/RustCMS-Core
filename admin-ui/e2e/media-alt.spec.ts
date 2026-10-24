import { expect, signIn, test } from "./fixtures";

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

// A file's record: renamed and described (alternative text) by its uploader.
test("a media file is renamed and gets an alternative text", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/media");
  await page.getByTestId("media-upload-input").setInputFiles({ name: "to-rename.png", mimeType: "image/png", buffer: PNG });
  await expect(page.getByRole("region", { name: "Uploads" }).getByText("Uploaded")).toBeVisible();
  const card = page.getByRole("listitem").filter({ hasText: "to-rename.png" });
  await card.getByRole("link", { name: "View" }).click();
  await expect(page.getByRole("heading", { name: "to-rename.png" })).toBeVisible();
  await expect(page.getByText("image/png · 68 bytes · 1 × 1")).toBeVisible();

  await page.getByRole("link", { name: "Edit" }).click();
  await page.getByLabel("Name").fill("../renamed.png");
  await page.getByLabel("Alternative text").fill("A single grey pixel");
  await page.getByRole("button", { name: "Save changes" }).click();
  // Cleaned like an uploaded name; the file shows with its new text.
  await expect(page.getByRole("heading", { name: "renamed.png" })).toBeVisible();
  await expect(page.getByRole("main").getByRole("img", { name: "A single grey pixel" })).toBeVisible();
});
