import { PASSWORD, expect, signIn, test } from "./fixtures";

test("admin creates a role and a user who then sees only what the role allows", async ({ page, browser, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Roles", exact: true }).click();
  await page.getByRole("link", { name: "New role" }).click();
  await page.getByLabel("Name").fill("reader");
  await page.getByRole("checkbox", { name: "Posts: List" }).check();
  await page.getByRole("checkbox", { name: "Posts: View" }).check();
  await page.getByRole("button", { name: "Create role" }).click();
  await expect(page.getByRole("heading", { name: "reader" })).toBeVisible();

  await page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Users", exact: true }).click();
  await page.getByRole("link", { name: "New user" }).click();
  await page.getByLabel("Name").fill("Rita Reader");
  await page.getByLabel("Email").fill("rita@example.test");
  await page.getByLabel("Password").fill("short");
  await page.getByRole("checkbox", { name: "reader" }).check();
  await page.getByRole("button", { name: "Create user" }).click();
  await expect(page.getByText(/at least 8 characters/)).toBeVisible();
  await page.getByLabel("Password").fill("readers password");
  await page.getByRole("button", { name: "Create user" }).click();
  await expect(page.getByRole("heading", { name: /Rita Reader/ })).toBeVisible();

  const context = await browser.newContext();
  const rita = await context.newPage();
  await rita.goto("/admin/login");
  await rita.getByLabel("Email").fill("rita@example.test");
  await rita.getByLabel("Password").fill("readers password");
  await rita.getByRole("button", { name: "Sign in" }).click();
  const nav = rita.getByRole("navigation", { name: "Resources" });
  await expect(nav.getByRole("link", { name: "Posts" })).toBeVisible();
  await expect(nav.getByRole("link", { name: "Categories" })).toHaveCount(0);
  await expect(nav.getByRole("link", { name: "Users" })).toHaveCount(0);
  await nav.getByRole("link", { name: "Posts" }).click();
  await expect(rita.getByRole("button", { name: /New post/ })).toHaveCount(0);

  // Disabling Rita signs her out at once.
  await page.getByRole("switch", { name: "May sign in" }).click();
  await expect(page.getByText("User disabled and signed out everywhere.")).toBeVisible();
  await rita.reload();
  await expect(rita.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await context.close();
});

test("editors have no system pages", async ({ page, watch: _ }) => {
  await signIn(page, "editor@example.test");
  await expect(page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Users", exact: true })).toHaveCount(0);
  await page.goto("/admin/system/users");
  await expect(page.getByRole("heading", { name: "Access denied" })).toBeVisible();
  await page.goto("/admin/system/roles");
  await expect(page.getByRole("heading", { name: "Access denied" })).toBeVisible();
});

test("changing the password keeps this session and ends the others", async ({ page, browser, watch: _ }) => {
  const other = await browser.newContext();
  const elsewhere = await other.newPage();
  await signIn(elsewhere, "editor@example.test");
  await signIn(page, "editor@example.test");
  await page.getByRole("link", { name: "Eddie Editor" }).click();
  await page.getByLabel("Current password").fill(PASSWORD);
  await page.getByLabel("New password", { exact: true }).fill("a new editor password");
  await page.getByLabel("Confirm new password").fill("a new editor password");
  await page.getByRole("button", { name: "Change password" }).click();
  await expect(page.getByText(/Password changed/)).toBeVisible();
  await page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Posts", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Posts" })).toBeVisible();
  await elsewhere.reload();
  await expect(elsewhere.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await other.close();
});
