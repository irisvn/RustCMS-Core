import { expect, signIn, test } from "./fixtures";

// System → Schedules and the Queue's history (0.8.0): Run now on a task and on
// a job schedule, their outcome without a reload, and the finished jobs.
test("an admin runs schedules now and follows their runs and the jobs' history", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/system/schedules");
  const table = page.getByRole("table", { name: "Schedules" });
  const digest = table.getByRole("row").filter({ hasText: "posts-digest" });
  await expect(digest).toBeVisible();
  await digest.getByRole("button", { name: "Run now" }).click();
  await page.getByRole("dialog", { name: "Run posts-digest now?" }).getByRole("button", { name: "Run now" }).click();
  await expect(page.getByText("posts-digest was started; its run appears below.")).toBeVisible();
  // A worker runs it; auto refresh shows the outcome and the task's summary.
  await expect(digest.getByText("Succeeded")).toBeVisible({ timeout: 20_000 });
  await expect(digest.getByText(/^\d+ posts$/)).toBeVisible();
  await table.getByRole("button", { name: "posts-digest" }).click();
  const runs = page.getByRole("dialog", { name: "Runs of posts-digest" });
  await expect(runs.getByText("Run now")).toBeVisible();
  await runs.getByRole("button", { name: "Close" }).click();

  const ping = table.getByRole("row").filter({ hasText: "posts-ping" });
  await ping.getByRole("button", { name: "Run now" }).click();
  await page.getByRole("dialog", { name: "Run posts-ping now?" }).getByRole("button", { name: "Run now" }).click();
  await expect(ping.getByText("Succeeded")).toBeVisible({ timeout: 20_000 });

  await page.goto("/admin/system/queue");
  await page.getByRole("tab", { name: "History" }).click();
  const history = page.getByRole("table", { name: "History" });
  await expect(history.getByText("count the posts").first()).toBeVisible();
  await expect(history.getByText("run schedule posts-digest").first()).toBeVisible();
  await history.getByRole("button", { name: "demo.ping_posts" }).first().click();
  await expect(page.getByRole("dialog", { name: "Job details" }).getByText("Done").first()).toBeVisible();
});
