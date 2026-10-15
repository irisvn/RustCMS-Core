import { readFile } from "node:fs/promises";
import { expect, signIn, test } from "./fixtures";

// The posts list exported as CSV, as shown (search applied).
test("exports the shown list as a CSV file", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts?search=Post+number+1");
  await expect(page.getByRole("heading", { name: "Posts", exact: true })).toBeVisible();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: /Export CSV/ }).click();
  const file = await download;
  expect(file.suggestedFilename()).toMatch(/^posts-\d{4}-\d{2}-\d{2}\.csv$/);
  const text = await readFile((await file.path())!, "utf8");
  const lines = text.split("\r\n").filter((l) => l !== "");
  expect(lines[0]).toContain("title");
  expect(lines.slice(1).every((l) => l.includes("Post number 1"))).toBe(true);
  expect(lines.length).toBeGreaterThan(1);
});
