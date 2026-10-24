import { expect, signIn, test } from "./fixtures";
import { createPost, trashPost } from "./records";

// An action's outcome: its message, and its page opened in a new tab once committed.
test("an action shows its message and opens its page in a new tab", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  const key = await createPost(page, "Outcome preview", false);
  await page.goto("/admin/r/posts?search=Outcome+preview");
  const row = page.getByRole("row").filter({ has: page.getByRole("cell", { name: "Outcome preview", exact: true }) });
  await row.getByRole("button", { name: "More actions for Outcome preview" }).click();
  const opened = page.context().waitForEvent("page");
  await page.getByRole("menuitem", { name: "Preview" }).click();
  const tab = await opened;
  await tab.waitForLoadState();
  expect(tab.url()).toMatch(new RegExp(`/admin/r/posts/records/${key}$`));
  // No way back to the panel's window.
  expect(await tab.evaluate(() => window.opener)).toBeNull();
  await expect(page.getByText(/^The preview opens in a new tab\./)).toBeVisible();
  await expect(page.getByRole("link", { name: "Open the page" })).toHaveAttribute("rel", "noopener noreferrer");
  await tab.close();
  await trashPost(page, key);
});
