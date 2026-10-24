import { expect, signIn, test } from "./fixtures";

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

// Rich text version 2 in the real panel. The CSP watcher of `fixtures.ts`
// fails the test on any violation: tables, figures, embeds and callouts must
// render without a `style` attribute, an iframe or a remote image.
test("the editor's v2 nodes, multi-image upload and the media library picker", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts/create");
  await page.getByLabel("Title").fill("Rich post");
  const editor = page.getByRole("textbox", { name: "Content" });
  await editor.click();

  // Heading 4 and a horizontal rule.
  await page.keyboard.type("Đà Nẵng");
  await page.getByRole("button", { name: "Heading 4" }).click();
  await page.keyboard.press("End");
  await page.keyboard.press("Enter");
  await page.getByRole("button", { name: "Horizontal rule" }).click();

  // Two images uploaded at once through the picker, inserted in order.
  await page.getByRole("button", { name: "Insert image" }).click();
  const picker = page.getByRole("dialog", { name: "Insert images" });
  await picker.getByRole("tab", { name: "Upload" }).click();
  await picker.locator("input[type=file]").setInputFiles([
    { name: "one.png", mimeType: "image/png", buffer: PNG },
    { name: "two.png", mimeType: "image/png", buffer: PNG },
  ]);
  await expect(picker.getByText("2 of at most 20 chosen")).toBeVisible();
  await picker.getByRole("button", { name: "Insert" }).click();
  await expect(editor.locator("figure[data-rich-image] img")).toHaveCount(2);

  // Image details: alternative text, caption, width.
  await editor.locator("figure[data-rich-image] img").first().click();
  await page.getByRole("button", { name: "Image details" }).click();
  const details = page.getByRole("dialog", { name: "Image details" });
  await details.getByLabel("Alternative text").fill("A pixel");
  await details.getByLabel("Caption").fill("One pixel");
  await details.getByRole("radio", { name: "Wide" }).check();
  await details.getByRole("button", { name: "Apply" }).click();
  await expect(editor.locator("figure[data-width=wide] figcaption")).toHaveText("One pixel");

  // A table with a header row, grown by a row.
  await editor.locator("p").last().click();
  await page.getByRole("button", { name: "Insert table" }).click();
  await page.keyboard.type("Plan");
  const tools = page.getByRole("toolbar", { name: "Table" });
  await tools.getByRole("button", { name: "Add row" }).click();
  await expect(editor.locator("table tr")).toHaveCount(4);

  // An embed: a text card, no iframe.
  await editor.locator("p").last().click();
  await page.getByRole("button", { name: "Embed a video" }).click();
  const embed = page.getByRole("dialog", { name: "Embed a video" });
  await embed.getByLabel("Address").fill("https://youtu.be/dQw4w9WgXcQ");
  await embed.getByRole("button", { name: "Insert" }).click();
  await expect(editor.locator("figure[data-embed]")).toContainText("YouTube · dQw4w9WgXcQ");

  // A callout.
  await editor.locator("p").last().click();
  await page.keyboard.type("Careful");
  await page.getByRole("button", { name: "Warning callout" }).click();
  await expect(editor.locator("aside[data-tone=warn]")).toContainText("Careful");

  // The cover from the library: one of the images just uploaded.
  await page.getByRole("button", { name: "Media library" }).click();
  const library = page.getByRole("dialog", { name: "Cover" });
  await library.getByRole("button", { name: /^two\.png/ }).first().click();
  await library.getByRole("button", { name: "Use" }).click();
  await expect(page.getByLabel("Cover").getByRole("img", { name: "two.png" })).toBeVisible();

  expect(await page.locator("[style]").count(), "no inline styles").toBe(0);
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: "Rich post" })).toBeVisible();

  const main = page.getByRole("main");
  await expect(main.locator("h4", { hasText: "Đà Nẵng" })).toBeVisible();
  await expect(main.locator("hr")).toHaveCount(1);
  await expect(main.locator("figure[data-width=wide] figcaption")).toHaveText("One pixel");
  await expect(main.locator("thead th", { hasText: "Plan" })).toBeVisible();
  await expect(main.getByRole("link", { name: "https://www.youtube.com/watch?v=dQw4w9WgXcQ" })).toBeVisible();
  await expect(main.locator("iframe")).toHaveCount(0);
  await expect(main.locator("aside[data-tone=warn]")).toContainText("Careful");
  for (const src of await main.locator("img").evaluateAll((list) => list.map((img) => img.getAttribute("src")))) {
    expect(src).toMatch(/^\/admin\/api\/media\/[0-9a-f-]{36}\/file$/);
  }

  // Leave no record behind for the other specs.
  await page.goto("/admin/r/posts?search=Rich+post");
  await page.getByRole("row", { name: /Rich post/ }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await page.getByLabel("Trashed records").selectOption("only");
  await page.getByRole("row", { name: /Rich post/ }).getByRole("button", { name: "Delete permanently" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Deleted permanently.")).toBeVisible();
});
