import { expect, signIn, test } from "./fixtures";

// Tags imported from a CSV file: one new, one known (matched by name), one
// refused; the report names the row, never its values.
test("imports tags from a CSV file, with a report of the rows refused", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/tags");
  await page.getByRole("link", { name: /Import CSV/ }).click();
  await expect(page.getByRole("heading", { name: "Import Tags" })).toBeVisible();
  await page.getByLabel("CSV file").setInputFiles({
    name: "tags.csv",
    mimeType: "text/csv",
    buffer: Buffer.from("name\r\nE2E imported\r\nRust\r\n\"\"\r\n"),
  });
  await expect(page.getByText("3 rows")).toBeVisible();
  await expect(page.getByRole("combobox", { name: "name" })).toHaveValue("name");
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByText("2 of 3 rows can be imported.")).toBeVisible();
  await page.getByRole("button", { name: "Import" }).click();
  await expect(page.getByText("Done: 1 created, 1 updated, 1 failed.")).toBeVisible({ timeout: 15_000 });
  await expect(page.getByRole("cell", { name: "is required" })).toBeVisible();

  // Leave nothing behind.
  await page.goto("/admin/r/tags?search=E2E+imported");
  await page.getByRole("row", { name: /E2E imported/ }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByRole("row", { name: /E2E imported/ })).toHaveCount(0);
});
