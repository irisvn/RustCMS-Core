import { expect, signIn, test } from "./fixtures";

// A post's tags chosen in its own form (ADR 0035), the rich text link dialog,
// and the question before leaving a form with unsaved changes. Leaves nothing
// behind.
test("tags are linked from the post's form; links are checked; unsaved changes are kept", async ({ page, watch: _ }) => {
  const run = Date.now().toString(36);
  const title = `Form links ${run}`;
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts/create");
  await page.getByLabel("Title").fill(title);

  // A link in the rich text: refused unless http(s) or mailto, then applied.
  const editor = page.getByRole("textbox", { name: "Content" });
  await editor.click();
  await editor.pressSequentially("Read more");
  await editor.press("ControlOrMeta+a");
  await page.getByRole("button", { name: "Link" }).click();
  const link = page.getByRole("dialog", { name: "Add link" });
  await link.getByLabel("Link address").fill("javascript:alert(1)");
  await link.getByRole("button", { name: "Apply" }).click();
  await expect(link.getByText("Links must start with https://, http:// or mailto:.")).toBeVisible();
  await link.getByLabel("Link address").fill("https://example.com/more");
  await link.getByRole("button", { name: "Apply" }).click();
  await expect(link).toBeHidden();
  await expect(editor.getByRole("link", { name: "Read more" })).toHaveAttribute("href", "https://example.com/more");

  // Two tags chosen before the post exists.
  await page.getByRole("tab", { name: "Publishing" }).click();
  for (const name of ["Rust", "Design"]) {
    await page.getByRole("combobox", { name: /Tags/ }).click();
    await page.getByRole("option", { name }).click();
  }
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  const url = page.url();
  const tags = page.getByRole("table", { name: "Tags" });
  await expect(tags.getByRole("row", { name: /Rust/ })).toBeVisible();
  await expect(tags.getByRole("row", { name: /Design/ })).toBeVisible();

  // Edit: one removed, one added, in one save.
  await page.goto(`${url}/edit`);
  await page.getByRole("tab", { name: "Publishing" }).click();
  await page.getByRole("button", { name: "Remove Design" }).click();
  await page.getByRole("combobox", { name: /Tags/ }).click();
  await page.getByRole("option", { name: "Security" }).click();
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  await expect(tags.getByRole("row", { name: /Security/ })).toBeVisible();
  await expect(tags.getByRole("row", { name: /Rust/ })).toBeVisible();
  await expect(tags.getByRole("row", { name: /Design/ })).toHaveCount(0);

  // Leaving with unsaved changes asks first; keeping them stays on the form.
  await page.goto(`${url}/edit`);
  await page.getByLabel("Title").fill(`${title} renamed`);
  const posts = page.getByRole("navigation").getByRole("link", { name: "Posts", exact: true });
  await posts.click();
  const leave = page.getByRole("dialog", { name: "Discard unsaved changes?" });
  await leave.getByRole("button", { name: "Keep editing" }).click();
  await expect(leave).toBeHidden();
  await expect(page.getByLabel("Title")).toHaveValue(`${title} renamed`);
  await posts.click();
  await page.getByRole("dialog", { name: "Discard unsaved changes?" }).getByRole("button", { name: "Discard changes" }).click();
  await expect(page).toHaveURL(/\/admin\/r\/posts$/);

  // Leave nothing behind: trash, then delete for good (its links go with it).
  await page.goto(`/admin/r/posts?search=${encodeURIComponent(title)}`);
  await page.getByRole("row", { name: title }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByRole("row", { name: title })).toHaveCount(0);
  await page.getByLabel("Trashed records").selectOption("only");
  await page.getByRole("row", { name: title }).getByRole("button", { name: "Delete permanently" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Deleted permanently.")).toBeVisible();
});
