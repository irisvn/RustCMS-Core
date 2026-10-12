import { type Page, expect, test as base } from "@playwright/test";

export const PASSWORD = "password123";
export { expect };

/** Every page records CSP violations and console errors from before its first script. */
export const test = base.extend<{ watch: { violations: string[]; errors: string[] } }>({
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

export async function signIn(page: Page, email: string) {
  await page.goto("/admin/login");
  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("navigation", { name: "Resources" })).toBeVisible();
}

