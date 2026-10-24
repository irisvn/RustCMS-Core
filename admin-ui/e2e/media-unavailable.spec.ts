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


const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

// A referenced file trashed later: shown as unavailable, never as a broken image.
test("a trashed file shows as unavailable where it is referenced", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts/create");
  await page.getByLabel("Title").fill("Post with a lost cover");
  await page.locator("input[type=file]").first().setInputFiles({ name: "lost-cover.png", mimeType: "image/png", buffer: PNG });
  await expect(page.getByRole("img", { name: "lost-cover.png" })).toBeVisible();
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: "Post with a lost cover" })).toBeVisible();
  const key = page.url().split("/").pop()!;
  // Trash its file in the media library.
  const post = (await (await page.request.get(`/admin/api/resources/posts/records/${key}`)).json()) as { data: { data: { cover: string } } };
  await adminApi(page, "DELETE", `resources/media/records/${post.data.data.cover}`);

  await page.goto(`/admin/r/posts/records/${key}`);
  await expect(page.getByRole("main").getByRole("img", { name: "File unavailable" })).toBeVisible();
  // Leave the seeded counts as other specs expect them.
  await adminApi(page, "DELETE", `resources/posts/records/${key}`);
});
