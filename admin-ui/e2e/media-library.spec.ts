import { expect, signIn, test } from "./fixtures";

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

// The Media page: thumbnails or the table (remembered per user), and an upload queue.
test("the media library uploads several files and shows them as a grid or a table", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/media");
  await expect(page.getByRole("heading", { name: "Media" })).toBeVisible();
  await page.getByTestId("media-upload-input").setInputFiles([
    { name: "grid-one.png", mimeType: "image/png", buffer: PNG },
    { name: "grid-two.png", mimeType: "image/png", buffer: PNG },
    { name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("plain text") },
  ]);
  const queue = page.getByRole("region", { name: "Uploads" });
  await expect(queue.getByText("Uploads finished")).toBeVisible();
  await expect(queue.getByText("2 files uploaded")).toBeVisible();
  // The server refuses the text file: no retry is offered (it would be refused again).
  await expect(queue.getByRole("alert")).toHaveCount(1);
  await expect(queue.getByRole("alert")).toContainText("not an accepted file type");
  await expect(queue.getByRole("button", { name: "Retry uploading notes.txt" })).toHaveCount(0);
  // Thumbnails: the authorized file URL, scaled by the browser.
  const thumb = page.getByRole("main").getByRole("img", { name: "grid-one.png" });
  await expect(thumb).toBeVisible();
  await expect(thumb).toHaveAttribute("src", /\/admin\/api\/media\/[0-9a-f-]{36}\/file$/);
  expect(await thumb.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBe(1);

  // The table instead, kept after a reload.
  await page.getByRole("button", { name: "List" }).click();
  await expect(page.getByRole("cell", { name: "grid-two.png" })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("button", { name: "List" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("cell", { name: "grid-two.png" })).toBeVisible();
  await page.getByRole("button", { name: "Grid" }).click();
  await expect(page.getByRole("main").getByRole("img", { name: "grid-two.png" })).toBeVisible();
});
