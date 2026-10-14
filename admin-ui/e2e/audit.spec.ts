import { expect, signIn, test } from "./fixtures";

test("an admin sees who changed a post, in the log and in its history", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  const nav = page.getByRole("navigation", { name: "Resources" });
  // Edit a seeded post and put it back: other specs count posts and titles.
  await page.goto("/admin/r/posts/records/3/edit");
  const title = page.getByLabel("Title");
  const original = await title.inputValue();
  await title.fill("Audited post");
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByRole("heading", { name: "Audited post" })).toBeVisible();

  await page.getByRole("button", { name: "Show history" }).click();
  const history = page.getByRole("list", { name: "Audit entries" });
  await expect(history.getByText("Updated")).toBeVisible();

  await nav.getByRole("link", { name: "Audit log" }).click();
  const log = page.getByRole("list", { name: "Audit entries" });
  await expect(log.getByText("Updated").first()).toBeVisible();
  await log.getByRole("button", { name: "Show details" }).first().click();
  await expect(log.getByText("Audited post").first()).toBeVisible();

  await page.goto("/admin/r/posts/records/3/edit");
  await page.getByLabel("Title").fill(original);
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByRole("heading", { name: original })).toBeVisible();
});

test("editors have no audit log", async ({ page, watch: _ }) => {
  await signIn(page, "editor@example.test");
  await expect(page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Audit log" })).toHaveCount(0);
  await page.goto("/admin/system/audit");
  await expect(page.getByRole("heading", { name: "Access denied" })).toBeVisible();
});
