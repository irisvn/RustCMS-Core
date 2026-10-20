import { expect, signIn, test } from "./fixtures";

// The Roles page (P7): areas, search, metadata and a preset declared by the demo.
test("an admin finds permissions by search and starts a role from a preset", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/system/roles/create");
  const area = (name: string) => page.locator("button[aria-controls]", { hasText: name });
  await expect(area("Live")).toBeVisible();
  await expect(area("Resources")).toBeVisible();
  // The declared label and description, found without diacritics.
  await page.getByRole("searchbox", { name: "Search permissions" }).fill("live demo");
  await expect(page.getByRole("checkbox", { name: /Use live demos/ })).toBeVisible();
  await expect(page.getByText("Ticks, echo socket and hello page")).toBeVisible();
  await expect(area("Resources")).toHaveCount(0);
  await page.getByRole("searchbox", { name: "Search permissions" }).fill("");

  await page.getByRole("button", { name: "Start from preset" }).click();
  await page.getByRole("menuitem", { name: /Viewer/ }).click();
  await expect(page.getByRole("status")).toContainText("Preset “Viewer” applied: 4 added, 0 removed.");
  await expect(page.getByRole("checkbox", { name: "Posts: List", exact: true })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: "Posts: Create", exact: true })).not.toBeChecked();
  await page.getByLabel("Name").fill("e2e-viewer");
  await page.getByRole("button", { name: "Create role" }).click();
  await expect(page.getByRole("heading", { name: "e2e-viewer" })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("checkbox", { name: "Posts: List", exact: true })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: "Categories: View", exact: true })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: "Posts: Update", exact: true })).not.toBeChecked();
});
