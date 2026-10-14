import { en } from "../src/i18n/en";
import { vi } from "../src/i18n/vi";
import type { Page } from "@playwright/test";
import { expect, signIn, test } from "./fixtures";

/** Visible text of the page that is English where Vietnamese differs (a missed translation). */
async function untranslated(page: Page): Promise<string[]> {
  const english = new Set<string>(
    Object.entries(en as Record<string, string>)
      .filter(([key, text]) => (vi as Record<string, string>)[key] !== text && !/\{/.test(text) && text.length > 3)
      .map(([, text]) => text),
  );
  const shown = await page.evaluate(() => {
    const texts: string[] = [];
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
      const node = walker.currentNode;
      const element = node.parentElement;
      if (element && element.closest("[hidden]") === null && node.textContent?.trim()) texts.push(node.textContent.trim());
    }
    for (const element of document.querySelectorAll("[aria-label],[placeholder],[title]")) {
      for (const name of ["aria-label", "placeholder", "title"]) {
        const value = element.getAttribute(name);
        if (value) texts.push(value);
      }
    }
    return texts;
  });
  return [...new Set(shown.filter((text: string) => english.has(text)))];
}

test("the panel speaks Vietnamese once chosen, on every device", async ({ page, browser, watch: _ }) => {
  test.setTimeout(90_000);
  await signIn(page, "admin@example.test");
  try {
    await page.goto("/admin/account");
    await page.getByLabel("Language").selectOption("vi");
    await expect(page.getByRole("heading", { name: "Tài khoản của bạn" })).toBeVisible();
    await expect(page.locator("html")).toHaveAttribute("lang", "vi");

    // Navigation and labels of the app (lang/vi.toml) and of the framework.
    const nav = page.getByRole("navigation", { name: "Danh mục quản lý" });
    await expect(nav.getByRole("link", { name: "Người dùng" })).toBeVisible();
    await nav.getByRole("link", { name: "Bài viết", exact: true }).click();
    await expect(page.getByRole("button", { name: "Thêm bài viết" })).toBeVisible();
    await expect(page.getByText(/Trang 1\/2 · 30 bản ghi/)).toBeVisible();
    expect(await untranslated(page)).toEqual([]);

    // Form: tabs, client and server validation messages.
    await page.getByRole("button", { name: "Thêm bài viết" }).click();
    await expect(page.getByRole("tab", { name: "Nội dung" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Thuộc tính" })).toBeVisible();
    await page.getByRole("button", { name: "Tạo", exact: true }).click();
    await expect(page.getByText("không được để trống")).toBeVisible();
    await page.getByRole("textbox", { name: /^Tiêu đề/ }).fill("x".repeat(201));
    await page.getByRole("button", { name: "Tạo", exact: true }).click();
    await expect(page.getByText("không được dài quá 200 ký tự")).toBeVisible();
    expect(await untranslated(page)).toEqual([]);

    // This device remembers the choice before signing in.
    await page.context().clearCookies();
    await page.goto("/admin/login");
    await expect(page.getByRole("heading", { name: "Đăng nhập" })).toBeVisible();

    // Saved on the account: another device (English browser) signs in to Vietnamese.
    const other = await browser.newContext({ locale: "en-US" });
    const device = await other.newPage();
    try {
      await signIn(device, "admin@example.test", /^Xin chào/);
      for (const path of ["/admin/system/users", "/admin/system/roles", "/admin/r/categories", "/admin/r/media"]) {
        await device.goto(path);
        await expect(device.getByRole("main").getByRole("heading").first()).toBeVisible();
        expect(await untranslated(device), path).toEqual([]);
      }
    } finally {
      await other.close();
    }
  } finally {
    // Other specs sign in as this user (and on this device): back to English.
    await signIn(page, "admin@example.test", /^(Welcome|Xin chào)/);
    await page.goto("/admin/account");
    await page.getByLabel(/Language|Ngôn ngữ/).selectOption("en");
    await expect(page.getByRole("heading", { name: "Your account" })).toBeVisible();
  }
});

test("before signing in, the browser's language is used", async ({ browser }) => {
  const context = await browser.newContext({ locale: "vi-VN" });
  const page = await context.newPage();
  await page.goto("http://localhost:8123/admin/login");
  await expect(page.getByRole("heading", { name: "Đăng nhập" })).toBeVisible();
  await page.getByLabel("Ngôn ngữ").selectOption("en");
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await context.close();
});
