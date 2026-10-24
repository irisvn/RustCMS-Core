import { expect, signIn, test } from "./fixtures";

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

// The CSP watcher of `fixtures.ts` fails the test on any violation: the editor
// must work without injected styles.
test("a cover image and rich text with an image are saved and shown", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts/create");
  await page.getByLabel("Title").fill("Media post");

  const pickers = page.locator("input[type=file]");
  // A file the field does not accept is refused before it is sent.
  await pickers.first().setInputFiles({ name: "doc.svg", mimeType: "image/svg+xml", buffer: Buffer.from("<svg/>") });
  await expect(page.getByText(/not accepted|not an accepted file type/)).toBeVisible();
  await pickers.first().setInputFiles({ name: "cover.png", mimeType: "image/png", buffer: PNG });
  await expect(page.getByRole("img", { name: "cover.png" })).toBeVisible();

  const editor = page.getByRole("textbox", { name: "Content" });
  await editor.click();
  await page.keyboard.type("Hello ");
  await page.getByRole("button", { name: "Bold" }).click();
  await page.keyboard.type("world");
  await page.getByRole("button", { name: "Bulleted list" }).click();
  await page.keyboard.type("an item");
  // Images go through the media library picker's upload tab.
  await page.getByRole("button", { name: "Insert image" }).click();
  const picker = page.getByRole("dialog", { name: "Insert images" });
  await picker.getByRole("tab", { name: "Upload" }).click();
  await picker.locator("input[type=file]").setInputFiles({ name: "inline.png", mimeType: "image/png", buffer: PNG });
  await expect(picker.getByText("1 of at most 20 chosen")).toBeVisible();
  await picker.getByRole("button", { name: "Insert" }).click();
  await expect(editor.locator("img")).toHaveCount(1);

  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: "Media post" })).toBeVisible();
  const main = page.getByRole("main");
  await expect(main.locator("strong", { hasText: "world" })).toBeVisible();
  await expect(main.locator("li", { hasText: "an item" })).toBeVisible();
  // Cover and inline image, both served by the panel itself.
  const images = main.locator("img");
  await expect(images).toHaveCount(2);
  for (const src of await images.evaluateAll((list) => list.map((img) => (img as HTMLImageElement).getAttribute("src")))) {
    expect(src).toMatch(/^\/admin\/api\/media\/[0-9a-f-]{36}\/file$/);
  }
  const loaded = await images.evaluateAll((list) => list.every((img) => (img as HTMLImageElement).naturalWidth === 1));
  expect(loaded).toBe(true);

  // Leave no record behind for the other specs (trash, then delete for good).
  await page.goto("/admin/r/posts?search=Media+post");
  await page.getByRole("row", { name: /Media post/ }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await page.getByLabel("Trashed records").selectOption("only");
  await page.getByRole("row", { name: /Media post/ }).getByRole("button", { name: "Delete permanently" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Deleted permanently.")).toBeVisible();
});
