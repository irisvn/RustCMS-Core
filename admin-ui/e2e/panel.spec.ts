import { type Page, expect, test as base } from "@playwright/test";

const PASSWORD = "password123";

/** Every page records CSP violations and console errors from before its first script. */
const test = base.extend<{ watch: { violations: string[]; errors: string[] } }>({
  watch: async ({ page }, use) => {
    const watch = { violations: [] as string[], errors: [] as string[] };
    await page.addInitScript(() => {
      document.addEventListener("securitypolicyviolation", (e) => {
        (window as unknown as { __csp: string[] }).__csp ??= [];
        (window as unknown as { __csp: string[] }).__csp.push(`${e.violatedDirective} ${e.blockedURI}`);
      });
    });
    page.on("console", (msg) => {
      if (msg.type() === "error" && !/401|403|404|422|Failed to load resource/.test(msg.text())) watch.errors.push(msg.text());
    });
    page.on("pageerror", (err) => watch.errors.push(err.message));
    await use(watch);
    const csp = await page.evaluate(() => (window as unknown as { __csp?: string[] }).__csp ?? []).catch(() => []);
    expect(csp, "CSP violations").toEqual([]);
    expect(watch.errors, "console errors").toEqual([]);
  },
});

async function signIn(page: Page, email: string) {
  await page.goto("/admin/login");
  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("navigation", { name: "Resources" })).toBeVisible();
}

test("admin manages posts end to end", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.getByRole("link", { name: "Posts" }).click();
  await expect(page.getByRole("heading", { name: "Posts" })).toBeVisible();
  await expect(page.getByText("Page 1 of 2 · 30 records")).toBeVisible();

  // Search, sort, filter and paginate through the URL-backed list state.
  await page.getByLabel("Search").fill("number 7");
  await expect(page.getByRole("cell", { name: "Post number 7", exact: true })).toBeVisible();
  await expect(page).toHaveURL(/search=number\+7/);
  await page.getByLabel("Search").fill("");
  await page.getByRole("button", { name: "Title" }).click();
  await expect(page).toHaveURL(/sort=title/);
  await page.getByRole("combobox", { name: "Published" }).selectOption("false");
  await expect(page.getByText("15 records")).toBeVisible();
  await page.getByRole("combobox", { name: "Published" }).selectOption("");
  await page.getByRole("button", { name: "Next" }).click();
  await expect(page.getByText("Page 2 of 2")).toBeVisible();

  // Create with a relationship select (options come from the server).
  await page.getByRole("button", { name: "New post" }).click();
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByText("is required")).toBeVisible();
  await page.getByLabel("Title").fill("Hello from Playwright");
  await page.getByLabel("Body").fill("Written by a robot.");
  await page.getByRole("combobox").click();
  await page.getByPlaceholder("Search…").fill("Gui");
  await page.getByRole("button", { name: "Guides" }).click();
  await page.getByRole("switch", { name: "Published" }).click();
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: "Hello from Playwright" })).toBeVisible();
  await expect(page.getByRole("combobox")).toHaveText(/Guides/);

  // Edit only what changed.
  await page.getByRole("link", { name: "Edit" }).click();
  await page.getByLabel("Title").fill("Hello again");
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByRole("heading", { name: "Hello again" })).toBeVisible();
  await expect(page.getByLabel("Body")).toHaveValue("Written by a robot.");

  // Delete after confirming.
  await page.getByRole("button", { name: "Delete" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();
  await expect(page.getByRole("heading", { name: "Posts" })).toBeVisible();
  await page.getByLabel("Search").fill("Hello again");
  await expect(page.getByText("No records found.")).toBeVisible();
});

test("bulk action reports its outcome", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts?filter%5Bpublished%5D%5Beq%5D=false&per_page=10");
  await expect(page.getByText("15 records")).toBeVisible();
  await page.getByLabel("Select all").check();
  await page.getByRole("button", { name: "Publish", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("Publish the selected posts?");
  await page.getByRole("dialog").getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByRole("alert")).toContainText("Publish: 10 records done.");
  await expect(page.getByText("5 records")).toBeVisible();
});

test("editor only sees and changes what policies allow", async ({ page, watch: _ }) => {
  await signIn(page, "editor@example.test");
  await page.getByRole("link", { name: "Posts" }).click();
  // Seeded: every third post belongs to the editor (10 of 30).
  await expect(page.getByText("10 records")).toBeVisible();
  await expect(page.getByRole("button", { name: "Delete" })).toHaveCount(0);
  // Categories are read-only for editors.
  await page.getByRole("link", { name: "Categories" }).click();
  await expect(page.getByRole("heading", { name: "Categories" })).toBeVisible();
  await expect(page.getByRole("button", { name: /New category/ })).toHaveCount(0);
  await expect(page.getByRole("link", { name: "Edit" })).toHaveCount(0);
  // Someone else's post is outside the editor's scope: not found, not "forbidden".
  await page.goto("/admin/r/posts/records/1");
  await expect(page.getByRole("heading", { name: "Not found" })).toBeVisible();
  await page.goto("/admin/r/categories/create");
  await expect(page.getByRole("heading", { name: "Access denied" })).toBeVisible();
});

test("session cookie is scoped and the session ends cleanly", async ({ page, context, watch: _ }) => {
  await signIn(page, "admin@example.test");
  const cookie = (await context.cookies()).find((c) => c.name === "rustcms_session");
  expect(cookie?.httpOnly).toBe(true);
  expect(cookie?.path).toBe("/admin");
  expect(cookie?.sameSite).toBe("Lax");

  // Reload, then write at once: the CSRF token is fetched again.
  await page.goto("/admin/r/posts/records/2/edit");
  await page.reload();
  await page.getByLabel("Title").fill("Edited right after reload");
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByRole("heading", { name: "Edited right after reload" })).toBeVisible();

  // Sign out; Back must not show data again.
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await expect(page.getByText("Edited right after reload")).toHaveCount(0);
});

test("an expired session sends back to sign-in and returns afterwards", async ({ page, context, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/categories");
  await expect(page.getByRole("heading", { name: "Categories" })).toBeVisible();
  await context.clearCookies();
  await page.goto("/admin/r/posts?page=2");
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await page.getByLabel("Email").fill("admin@example.test");
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/admin\/r\/posts\?page=2$/);
});

test("a crafted redirect cannot leave the panel", async ({ page, watch: _ }) => {
  await page.goto("/admin/login?redirect=%2F%2Fevil.example%2F");
  await page.getByLabel("Email").fill("admin@example.test");
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL("http://localhost:8123/admin/");
});

test("bad credentials show a field error", async ({ page, watch: _ }) => {
  await page.goto("/admin/login");
  await page.getByLabel("Email").fill("admin@example.test");
  await page.getByLabel("Password").fill("wrong password");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText("These credentials do not match our records.")).toBeVisible();
});
