import { type Page, expect } from "@playwright/test";

/** A write to the admin API as the signed-in user of `page` (CSRF token and Origin of the panel). */
export async function adminApi(page: Page, method: string, path: string, body?: unknown): Promise<unknown> {
  const csrf = (await (await page.request.get("/admin/api/auth/csrf")).json()) as { csrfToken: string };
  const origin = new URL(page.url()).origin;
  const response = await page.request.fetch(`/admin/api/${path}`, {
    method,
    data: body,
    headers: { "X-CSRF-Token": csrf.csrfToken, Origin: origin, "Content-Type": "application/json" },
  });
  expect(response.ok(), `${method} ${path}: ${response.status()}`).toBeTruthy();
  return response.status() === 204 ? null : response.json();
}

/** A post of this test only (the seeded ones are shared by every spec): its key. */
export async function createPost(page: Page, title: string, published: boolean): Promise<string> {
  const created = (await adminApi(page, "POST", "resources/posts/records", {
    title,
    published,
    published_at: published ? new Date().toISOString() : null,
  })) as { data: { key: string } };
  return created.data.key;
}

/** Moves the post to the trash (the seeded counts stay as other specs expect them). */
export async function trashPost(page: Page, key: string): Promise<void> {
  await adminApi(page, "DELETE", `resources/posts/records/${encodeURIComponent(key)}`);
}
