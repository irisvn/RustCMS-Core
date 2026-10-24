import type { Page } from "@playwright/test";
import { expect, signIn, test } from "./fixtures";

/** A write to the admin API as the signed-in user (its CSRF token and the panel's Origin). */
async function adminApi(page: Page, method: string, path: string, body?: unknown): Promise<unknown> {
  const csrf = (await (await page.request.get("/admin/api/auth/csrf")).json()) as { csrfToken: string };
  const response = await page.request.fetch(`/admin/api/${path}`, {
    method,
    data: body,
    headers: { "X-CSRF-Token": csrf.csrfToken, Origin: new URL(page.url()).origin, "Content-Type": "application/json" },
  });
  expect(response.ok(), `${method} ${path}: ${response.status()}`).toBeTruthy();
  return response.status() === 204 ? null : response.json();
}

/** A post of this test only (seeded posts are shared by every spec): its key. */
async function createPost(page: Page, title: string, published: boolean): Promise<string> {
  const body = { title, published, published_at: published ? new Date().toISOString() : null };
  return ((await adminApi(page, "POST", "resources/posts/records", body)) as { data: { key: string } }).data.key;
}


// Dates and times on a record's page: in the user's language and time zone, not wire text.
test("a record's page formats its dates and times", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  const key = await createPost(page, "Dated post", true);
  await page.goto(`/admin/r/posts/records/${key}`);
  await page.getByRole("main").getByRole("tab", { name: "Publishing" }).click();
  const shown = page.getByRole("main").locator("time:visible");
  await expect(shown.first()).toBeVisible();
  const text = (await shown.first().textContent()) ?? "";
  expect(text).not.toMatch(/\d{4}-\d{2}-\d{2}T/);
  expect(text).toMatch(/\d{4}/);
  expect(await shown.first().getAttribute("datetime")).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  await adminApi(page, "DELETE", `resources/posts/records/${key}`);
});
