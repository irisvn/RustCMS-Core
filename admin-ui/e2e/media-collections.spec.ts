import { expect, signIn, test } from "./fixtures";
import { adminApi } from "./records";

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

// Media collections (the demo declares post covers, post content and
// branding): a field's uploads go into its collection, its picker opens on it,
// the Media page filters by collection and uploads into the one it shows, and
// a file's record moves it to another.
test("files are sorted into collections and filtered by them", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  try {
    await page.goto("/admin/r/posts/create");
    await page.locator("input[type=file]").first().setInputFiles({ name: "coll-cover.png", mimeType: "image/png", buffer: PNG });
    await expect(page.getByRole("img", { name: "coll-cover.png" })).toBeVisible();

    // The cover's picker opens on its collection; every file is a choice away.
    await page.getByRole("button", { name: "Media library" }).first().click();
    const picker = page.getByRole("dialog").last();
    const collection = picker.getByLabel("Collection");
    await expect(collection).toHaveValue("post-covers");
    const files = picker.getByRole("list", { name: "Files" });
    await expect(files.getByRole("button", { name: /^coll-cover\.png/ })).toBeVisible();
    await collection.selectOption({ label: "Unassigned" });
    await expect(files.getByRole("button", { name: /^coll-cover\.png/ })).toHaveCount(0);
    await collection.selectOption({ label: "All files" });
    await expect(files.getByRole("button", { name: /^coll-cover\.png/ })).toBeVisible();
    await picker.getByRole("button", { name: "Cancel" }).click();

    // The Media page (its filters open): filtered on the covers, its uploads go there too.
    await page.goto("/admin/r/media");
    await page.getByLabel("Collection").selectOption({ label: "Post covers" });
    const main = page.getByRole("main");
    await expect(main.getByRole("img", { name: "coll-cover.png" })).toBeVisible();
    await page.getByTestId("media-upload-input").setInputFiles({ name: "coll-page.png", mimeType: "image/png", buffer: PNG });
    await expect(page.getByRole("region", { name: "Uploads" }).getByText("1 file uploaded")).toBeVisible();
    await expect(main.getByRole("img", { name: "coll-page.png" })).toBeVisible();
    await page.getByLabel("Collection").selectOption({ label: "Unassigned" });
    await expect(main.getByRole("img", { name: "coll-cover.png" })).toHaveCount(0);

    // A file's record moves it to another collection.
    await page.getByLabel("Collection").selectOption({ label: "Post covers" });
    await main.getByRole("listitem").filter({ hasText: "coll-page.png" }).getByRole("link", { name: "View" }).click();
    await expect(page.getByRole("heading", { name: "coll-page.png" })).toBeVisible();
    await page.getByRole("link", { name: "Edit" }).click();
    await expect(page.getByLabel("Collection")).toHaveValue(JSON.stringify("post-covers"));
    await page.getByLabel("Collection").selectOption({ label: "Branding" });
    await page.getByRole("button", { name: "Save changes" }).click();
    await expect(page.getByRole("heading", { name: "coll-page.png" })).toBeVisible();
    await page.goto("/admin/r/media");
    await page.getByLabel("Collection").selectOption({ label: "Branding" });
    await expect(main.getByRole("img", { name: "coll-page.png" })).toBeVisible();
    await expect(main.getByRole("img", { name: "coll-cover.png" })).toHaveCount(0);
  } finally {
    // Leave no file behind (trash, then delete for good).
    for (const name of ["coll-cover.png", "coll-page.png"]) {
      const found = (await adminApi(page, "GET", `resources/media/records?search=${name}&trashed=with`)) as { data: { key: string }[] };
      for (const { key } of found.data) {
        await page.request.fetch(`/admin/api/resources/media/records/${key}`, { method: "DELETE", failOnStatusCode: false, headers: await headers(page) });
        await adminApi(page, "DELETE", `resources/media/records/${key}/force`);
      }
    }
  }
});

async function headers(page: import("@playwright/test").Page) {
  const csrf = (await (await page.request.get("/admin/api/auth/csrf")).json()) as { csrfToken: string };
  return { "X-CSRF-Token": csrf.csrfToken, Origin: new URL(page.url()).origin };
}
