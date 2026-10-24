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


// Generated text: empty values, relation search boxes, the order of resources.
test("generated text is plain and resources keep their order", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  // The dashboard's resource cards: in registration order, not alphabetical.
  const cards = page.getByRole("main").getByRole("heading", { level: 3 });
  await expect(cards.first()).toBeVisible();
  const names = await cards.allTextContents();
  expect(names.indexOf("Media")).toBeLessThan(names.indexOf("Categories"));
  expect(names.indexOf("Posts")).toBeLessThan(names.indexOf("Integrations"));

  // A draft: no publication time, said in words.
  const key = await createPost(page, "Undated draft", false);
  await page.goto(`/admin/r/posts/records/${key}`);
  await page.getByRole("main").getByRole("tab", { name: "Publishing" }).click();
  await expect(page.getByRole("main").locator("dd:visible", { hasText: /^Not set$/ }).first()).toBeVisible();
  await expect(page.getByRole("main").getByText(/\(null\)/)).toHaveCount(0);
  await adminApi(page, "DELETE", `resources/posts/records/${key}`);

  // A relation's search box says what it searches.
  await page.goto("/admin/r/categories");
  await page.getByRole("row").nth(1).getByRole("link", { name: "View" }).click();
  await expect(page.getByRole("textbox", { name: "Search Posts" })).toHaveAttribute("placeholder", "Search posts…");
});
