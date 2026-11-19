import { type Page, expect, test as base } from "@playwright/test";

export const PASSWORD = "password123";
export { expect };

/** Every page records CSP violations and console errors from before its first script. */
export const test = base.extend<{ watch: { violations: string[]; errors: string[] } }>({
  watch: async ({ page }, use) => {
    const watch = { violations: [] as string[], errors: [] as string[] };
    // Keep violations across full-page navigations, not just the final document.
    await page.exposeBinding("__reportCsp", (_source, violation: string) => watch.violations.push(violation));
    await page.addInitScript(() => {
      document.addEventListener("securitypolicyviolation", (e) => {
        const violation = `${e.violatedDirective} ${e.blockedURI}`;
        (window as unknown as { __csp: string[] }).__csp ??= [];
        (window as unknown as { __csp: string[] }).__csp.push(violation);
        void (window as unknown as { __reportCsp: (violation: string) => Promise<void> }).__reportCsp(violation);
      });
    });
    page.on("console", (msg) => {
      if (msg.type() === "error" && !/401|403|404|422|Failed to load resource/.test(msg.text())) watch.errors.push(msg.text());
    });
    page.on("pageerror", (err) => watch.errors.push(err.message));
    await use(watch);
    const csp = await page.evaluate(() => (window as unknown as { __csp?: string[] }).__csp ?? []).catch(() => []);
    expect(csp, "CSP violations").toEqual([]);
    expect(watch.violations, "CSP violations across every visited page").toEqual([]);
    expect(watch.errors, "console errors").toEqual([]);
  },
});

export async function signIn(page: Page, email: string, welcome: RegExp = /^Welcome/) {
  await page.goto("/admin/login");
  // In whichever language this device chose before.
  await page.getByLabel(/^(Email)$/).fill(email);
  await page.getByLabel(/^(Password|Mật khẩu)$/).fill(PASSWORD);
  await page.getByRole("button", { name: /^(Sign in|Đăng nhập)$/ }).click();
  // Mobile navigation is intentionally closed after signing in.
  await expect(page.getByRole("main").getByRole("heading", { name: welcome })).toBeVisible();
}

/**
 * Colors as sRGB channels (0–255), resolved by the browser: a CSS color, or a
 * token as `--name` (var(), color-mix() and oklab() included; a canvas would
 * misread oklab). Through the CSSOM: no injected styles.
 */
export async function srgb(page: Page, colors: string[]): Promise<number[][]> {
  return page.evaluate((list) => list.map((value) => {
    const probe = document.createElement("span");
    document.body.append(probe);
    probe.style.color = `color-mix(in srgb, ${value.startsWith("--") ? `var(${value})` : value} 100%, transparent)`;
    const resolved = getComputedStyle(probe).color;
    probe.remove();
    const scale = resolved.startsWith("color(") ? 255 : 1;
    return (resolved.match(/-?\d*\.?\d+(?:e-?\d+)?/g) ?? []).slice(0, 3).map((n) => Math.round(Number(n) * scale));
  }), colors);
}

/** WCAG contrast of two sRGB colors (0–255). */
export function contrast(a: number[], b: number[]): number {
  const luminance = (rgb: number[]) => {
    const [r, g, b] = rgb.map((channel) => {
      const value = channel / 255;
      return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    });
    return r! * 0.2126 + g! * 0.7152 + b! * 0.0722;
  };
  const x = luminance(a), y = luminance(b);
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}
