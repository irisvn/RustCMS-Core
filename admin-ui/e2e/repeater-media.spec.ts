import { expect, signIn, test } from "./fixtures";
import { adminApi } from "./records";

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

// Files in repeater rows (ADR 0044, part B): the demo's footer links take an
// icon, uploaded in the row, checked with the page's files when saved, and
// shown again in its row. Nothing is left behind.
test("a row uploads its own file, saved with the settings", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  try {
    await page.goto("/admin/settings/site");
    const links = page.getByRole("group", { name: "Footer links" }).or(page.locator("#field-social_links"));
    await expect(links.getByText("No items yet.")).toBeVisible();
    await page.getByRole("button", { name: "Add a link" }).click();
    const row = links.getByRole("listitem").first();
    await row.getByLabel(/^Label/).fill("Docs");
    await row.getByLabel(/^Url/).fill("https://docs.example.test");
    await row.locator("input[type=file]").setInputFiles({ name: "row-icon.png", mimeType: "image/png", buffer: PNG });
    await expect(row.getByRole("img", { name: "row-icon.png" })).toBeVisible();
    await page.getByRole("button", { name: "Save settings" }).click();
    await expect(page.getByText("Settings saved.")).toBeVisible();

    // Stored in the row: shown again after a reload.
    await page.reload();
    await expect(links.getByRole("listitem").first().getByRole("button", { name: "View image" })).toBeVisible();

    // Leave nothing behind: the row, then the file.
    await links.getByRole("listitem").first().getByRole("button", { name: "Remove", exact: true }).click();
    await page.getByRole("button", { name: "Save settings" }).click();
    await expect(page.getByText("Settings saved.")).toBeVisible();
  } finally {
    const found = (await adminApi(page, "GET", "resources/media/records?search=row-icon.png&trashed=with")) as { data: { key: string }[] };
    for (const { key } of found.data) {
      await adminApi(page, "DELETE", `resources/media/records/${key}`).catch(() => undefined);
      await adminApi(page, "DELETE", `resources/media/records/${key}/force`);
    }
  }
});
