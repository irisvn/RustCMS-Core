import { expect, signIn, test } from "./fixtures";

// Runs after the other specs (workers: 1, files in order) and leaves no records behind.
test("a lost create is retried safely, then trashed, restored and deleted for good", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts/create");
  await page.getByLabel("Title").fill("Exactly once");

  // The server commits the first attempt, but its answer is lost on the way.
  let attempts = 0;
  const keys: (string | null)[] = [];
  await page.route("**/admin/api/resources/posts/records", async (route) => {
    if (route.request().method() !== "POST") return route.continue();
    attempts += 1;
    keys.push(route.request().headers()["idempotency-key"] ?? null);
    const response = await route.fetch();
    if (attempts === 1) return route.fulfill({ status: 504, body: "" });
    return route.fulfill({ response });
  });
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByText(/may not have been saved/)).toBeVisible();
  await expect(page.getByLabel("Title")).toBeDisabled();
  await page.getByRole("button", { name: "Retry safely" }).click();
  await expect(page.getByRole("heading", { name: "Exactly once" })).toBeVisible();
  expect(attempts).toBe(2);
  expect(keys[0]).toBeTruthy();
  expect(keys[1]).toBe(keys[0]);
  await page.unroute("**/admin/api/resources/posts/records");

  // One record, not two.
  await page.goto("/admin/r/posts?search=Exactly+once");
  await expect(page.getByText("1 record", { exact: true })).toBeVisible();

  // Delete moves it to the trash.
  await page.getByRole("row", { name: /Exactly once/ }).getByRole("button", { name: "Delete" }).click();
  await expect(page.getByText(/will be moved to the trash/)).toBeVisible();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Moved to the trash.")).toBeVisible();
  await expect(page.getByText("0 records", { exact: true })).toBeVisible();

  await page.getByLabel("Trashed records").selectOption("only");
  const row = page.getByRole("row", { name: /Exactly once/ });
  await expect(row.getByText("Trashed")).toBeVisible();
  await row.getByRole("button", { name: /Restore/ }).click();
  await expect(page.getByText("Restored.")).toBeVisible();
  await page.getByLabel("Trashed records").selectOption("");
  await expect(page.getByText("1 record", { exact: true })).toBeVisible();

  // Trash it again, then delete it for good.
  await page.getByRole("row", { name: /Exactly once/ }).getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Confirm" }).click();
  await page.getByLabel("Trashed records").selectOption("only");
  await page.getByRole("row", { name: /Exactly once/ }).getByRole("button", { name: "Delete permanently" }).click();
  await expect(page.getByText(/will be deleted permanently/)).toBeVisible();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Deleted permanently.")).toBeVisible();
  await expect(page.getByText("0 records", { exact: true })).toBeVisible();
});
