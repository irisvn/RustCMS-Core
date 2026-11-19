import { expect, signIn, test } from "./fixtures";

// Rows of rows (ADR 0044, part C), on the demo's categories: each section of
// the landing page holds its links. A new section and its links are saved at
// once; a link's note shows in a detailed section only (it reads the section
// it is in); removing a section removes its links. Nothing is left behind.
test("a category's sections hold their links, saved and removed with them", async ({ page, watch: _ }) => {
  const run = Date.now().toString(36);
  const name = `E2E sections ${run}`;
  await signIn(page, "admin@example.test");

  await page.goto("/admin/r/categories/create");
  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Slug").fill(`e2e-sections-${run}`);
  await page.getByRole("button", { name: "Add a section" }).click();
  // The section is the first row; its links are rows inside it.
  const section = page.getByRole("main").getByRole("listitem").first();
  // A reaction on the rows titles a new section (ADR 0044, part D); the user may change it.
  await expect(section.getByLabel(/^Title/)).toHaveValue("Section 1");
  await section.getByLabel(/^Title/).fill("Start here");
  const links = section.getByRole("listitem");
  await section.getByRole("button", { name: "Add a link" }).click();
  await section.getByRole("button", { name: "Add a link" }).click();
  await expect(links).toHaveCount(2);
  await links.nth(0).getByLabel(/^Label/).fill("Guide");
  await links.nth(0).getByLabel(/^URL/).fill("https://example.test/guide");
  await links.nth(1).getByLabel(/^Label/).fill("FAQ");
  await links.nth(1).getByLabel(/^URL/).fill("https://example.test/faq");
  // A compact section: no notes. Detailed: each link takes one.
  await expect(links.nth(0).getByLabel(/^Note/)).toHaveCount(0);
  await section.getByLabel(/^Style/).selectOption({ label: "Detailed" });
  await links.nth(0).getByLabel(/^Note/).fill("Read first");
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name })).toBeVisible();
  const url = page.url();

  // Stored: the edit form reads the section, then its links through it.
  await page.goto(`${url}/edit`);
  await expect(section.getByLabel(/^Title/)).toHaveValue("Start here");
  await expect(links).toHaveCount(2);
  await expect(links.nth(0).getByLabel(/^Label/)).toHaveValue("Guide");
  await expect(links.nth(0).getByLabel(/^Note/)).toHaveValue("Read first");
  await expect(links.nth(1).getByLabel(/^Label/)).toHaveValue("FAQ");

  // Removing the section removes its links.
  await section.getByRole("button", { name: "Remove", exact: true }).first().click();
  await expect(page.getByRole("main").getByRole("listitem")).toHaveCount(0);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page).toHaveURL(url);
  await page.goto(`${url}/edit`);
  await expect(page.getByRole("button", { name: "Add a section" })).toBeVisible();
  await expect(page.getByRole("main").getByLabel(/^Title/)).toHaveCount(0);

  // Leave nothing behind.
  await page.goto(`/admin/r/categories?search=${encodeURIComponent(name)}`);
  await page.getByRole("row", { name }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByRole("row", { name })).toHaveCount(0);
});
