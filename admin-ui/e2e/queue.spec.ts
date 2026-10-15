import { expect, signIn, test } from "./fixtures";

test("the queue page is in the System menu, for those who may see it", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Queue", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Queue" })).toBeVisible();
  // The demo dispatches no jobs: an empty queue, an empty list.
  await expect(page.getByText("No jobs in any queue.")).toBeVisible();
  await expect(page.getByRole("table", { name: "Jobs" })).toBeVisible();
  await page.getByLabel("Status").selectOption("failed");
  await page.getByRole("button", { name: "Filter" }).click();
  await expect(page.getByText("No jobs match.")).toBeVisible();
});

test("editors have no queue page", async ({ page, watch: _ }) => {
  await signIn(page, "editor@example.test");
  await expect(page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Posts" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Queue", exact: true })).toHaveCount(0);
});
