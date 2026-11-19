import { expect, signIn, test } from "./fixtures";

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

// Images open in the viewer from a form's field, the media picker (over its
// dialog), a record's page, its rich text and the media library; Escape closes
// only the viewer. The CSP watcher fails the test on an injected style.
test("images open in the viewer wherever they are shown", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts/create");
  await page.getByLabel("Title").fill("Viewer post");
  await page.locator("input[type=file]").first().setInputFiles({ name: "viewer-cover.png", mimeType: "image/png", buffer: PNG });
  const viewer = page.getByLabel("Image viewer");

  // The form's field: viewing never changes the value.
  await page.getByRole("button", { name: "View viewer-cover.png" }).click();
  await expect(viewer).toBeVisible();
  await expect(viewer.getByText("viewer-cover.png")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(viewer).toHaveCount(0);
  await expect(page.getByRole("img", { name: "viewer-cover.png" })).toBeVisible();

  // The picker: its magnifier views over the dialog; Escape closes the viewer only.
  await page.getByRole("button", { name: "Media library" }).first().click();
  const picker = page.getByRole("dialog").last();
  await picker.getByRole("button", { name: "View viewer-cover.png" }).first().click();
  await expect(viewer).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(viewer).toHaveCount(0);
  await expect(picker).toBeVisible();
  await picker.getByRole("button", { name: "Close" }).last().click();

  // An image in the rich text.
  const editor = page.getByRole("textbox", { name: "Content" });
  await editor.click();
  await page.keyboard.type("Look:");
  await page.getByRole("button", { name: "Insert image" }).click();
  const insert = page.getByRole("dialog", { name: "Insert images" });
  await insert.getByRole("tab", { name: "Upload" }).click();
  await insert.locator("input[type=file]").setInputFiles({ name: "viewer-inline.png", mimeType: "image/png", buffer: PNG });
  await expect(insert.getByText("1 of at most 20 chosen")).toBeVisible();
  await insert.getByRole("button", { name: "Insert" }).click();
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: "Viewer post" })).toBeVisible();

  // The record's page: the cover (details looked up), zoom, close.
  const main = page.getByRole("main");
  const views = main.getByRole("button", { name: /^View / });
  await expect(views).toHaveCount(2);
  await views.first().click();
  await expect(viewer).toBeVisible();
  await expect(viewer.getByText("viewer-cover.png")).toBeVisible();
  await viewer.getByRole("button", { name: "Zoom in" }).click();
  await expect(viewer.getByRole("button", { name: "Download" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(viewer).toHaveCount(0);
  // The document's image.
  await views.last().click();
  await expect(viewer).toBeVisible();
  await expect(viewer.getByText("viewer-inline.png")).toBeVisible();
  await page.keyboard.press("Escape");

  // The media library: the page's images, one after the other.
  await page.goto("/admin/r/media?search=viewer-");
  await page.getByRole("button", { name: /^View viewer-/ }).first().click();
  await expect(viewer).toBeVisible();
  await expect(viewer.getByText("1 / 2")).toBeVisible();
  await page.keyboard.press("ArrowRight");
  await expect(viewer.getByText("2 / 2")).toBeVisible();
  await viewer.getByRole("button", { name: "Previous image" }).click();
  await expect(viewer.getByText("1 / 2")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(viewer).toHaveCount(0);

  // Leave no record behind (trash, then delete for good).
  await page.goto("/admin/r/posts?search=Viewer+post");
  await page.getByRole("row", { name: /Viewer post/ }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await page.getByLabel("Trashed records").selectOption("only");
  await page.getByRole("row", { name: /Viewer post/ }).getByRole("button", { name: "Delete permanently" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Deleted permanently.")).toBeVisible();
});
