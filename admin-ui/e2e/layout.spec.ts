import { mkdir } from "node:fs/promises";
import { type Page } from "@playwright/test";
import type { PanelResponse } from "../src/api/types";
import { expect, signIn, test } from "./fixtures";

const screenshotDirectory = process.env.RUSTCMS_UI_SCREENSHOTS;
const viewports = [320, 390, 768, 1440];

async function fitsViewport(page: Page) {
  const layout = await page.evaluate(() => {
    const width = document.documentElement.clientWidth;
    // Wide tables may scroll inside their own container, never the document.
    const scrollers = [...document.querySelectorAll<HTMLElement>("body *")]
      .filter((element) => element.getClientRects().length > 0 && element.scrollWidth > element.clientWidth + 1
        && ["auto", "scroll"].includes(getComputedStyle(element).overflowX))
      .map((element) => ({ tag: element.tagName, containsTable: !!element.querySelector("table") }));
    return { width, documentWidth: document.documentElement.scrollWidth, bodyWidth: document.body.scrollWidth, scrollers };
  });
  expect.soft(layout.documentWidth, `${page.url()}: the document must not scroll horizontally`).toBeLessThanOrEqual(layout.width + 1);
  expect.soft(layout.bodyWidth, `${page.url()}: the body must not scroll horizontally`).toBeLessThanOrEqual(layout.width + 1);
  for (const scroller of layout.scrollers) expect(scroller.containsTable, `${scroller.tag} horizontal scroll is local to a table`).toBe(true);
}

async function expectContrast(page: Page) {
  const ratios = await page.evaluate(() => {
    const styles = getComputedStyle(document.documentElement);
    const context = document.createElement("canvas").getContext("2d")!;
    const luminance = (token: string) => {
      context.fillStyle = styles.getPropertyValue(token).trim();
      context.fillRect(0, 0, 1, 1);
      const channels = [...context.getImageData(0, 0, 1, 1).data].slice(0, 3).map((channel) => {
        const value = channel / 255;
        return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
      });
      return channels[0]! * 0.2126 + channels[1]! * 0.7152 + channels[2]! * 0.0722;
    };
    const contrast = (first: string, second: string) => {
      const a = luminance(first), b = luminance(second);
      return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
    };
    return {
      destructiveTextOnCard: contrast("--destructive", "--card"),
      destructiveTextOnBackground: contrast("--destructive", "--background"),
      destructiveButton: contrast("--destructive-foreground", "--destructive"),
      successTextOnCard: contrast("--success", "--card"),
      warningTextOnCard: contrast("--warning", "--card"),
      focusRingOnCard: contrast("--ring", "--card"),
      focusRingOnBackground: contrast("--ring", "--background"),
    };
  });
  for (const [name, ratio] of Object.entries(ratios)) {
    expect(ratio, `${name} WCAG contrast ratio`).toBeGreaterThanOrEqual(name.startsWith("focusRing") ? 3 : 4.5);
  }
}

async function capture(page: Page, name: string, width: number, theme: string) {
  if (!screenshotDirectory || ![390, 1440].includes(width)) return;
  await mkdir(screenshotDirectory, { recursive: true });
  await page.screenshot({ path: `${screenshotDirectory}/${name}-${width}-${theme}.png`, fullPage: true, animations: "disabled" });
}

for (const width of viewports) {
  for (const colorScheme of ["light", "dark"] as const) {
    test(`responsive pages at ${width}px in ${colorScheme} mode`, async ({ page, watch: _ }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await page.goto("/admin/login");
      await expect(page.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
      await expectContrast(page);
      await fitsViewport(page);
      await capture(page, "login", width, colorScheme);
      await signIn(page, "admin@example.test");
      await fitsViewport(page);
      const [red, green, blue] = await page.locator("body").evaluate((element) => {
        // Canvas resolves modern oklch colors to RGB without injecting CSP-blocked styles.
        const canvas = document.createElement("canvas");
        const context = canvas.getContext("2d")!;
        context.fillStyle = getComputedStyle(element).backgroundColor;
        context.fillRect(0, 0, 1, 1);
        return [...context.getImageData(0, 0, 1, 1).data];
      });
      // Both themes are monochrome, with a genuinely dark/light canvas.
      expect(Math.abs(red! - green!)).toBeLessThanOrEqual(2);
      expect(Math.abs(green! - blue!)).toBeLessThanOrEqual(2);
      if (colorScheme === "dark") expect(red!).toBeLessThan(80);
      else expect(red!).toBeGreaterThan(200);
      if (width < 768) {
        await expect(page.getByRole("navigation", { name: "Resources" })).toBeHidden();
        await expect(page.getByRole("button", { name: "Open navigation" })).toBeVisible();
      } else {
        await expect(page.getByRole("navigation", { name: "Resources" })).toBeVisible();
        await expect(page.getByRole("button", { name: "Open navigation" })).toBeHidden();
      }
      await capture(page, "home", width, colorScheme);

      for (const [name, path, heading] of [
        ["list", "/admin/r/posts", "Posts"],
        ["form", "/admin/r/posts/create", "New post"],
        ["edit", "/admin/r/posts/records/1/edit", "Edit Post number 1"],
        ["record", "/admin/r/posts/records/1", "Post number 1"],
        ["system-users", "/admin/system/users", "Users"],
        ["system-roles", "/admin/system/roles", "Roles"],
        ["system-user-form", "/admin/system/users/create", "New user"],
        ["system-role-form", "/admin/system/roles/create", "New role"],
        ["account", "/admin/account", "Your account"],
      ]) {
        await page.goto(path!);
        await expect(page.getByRole("main").getByRole("heading", { name: heading!, exact: true })).toBeVisible();
        if (name === "list" || name === "system-users" || name === "system-roles") {
          const table = page.getByRole("main").getByRole("table");
          await expect(table.getByRole("row").nth(1)).toBeVisible();
          if (width === 320 && name === "system-users") {
            expect(await table.evaluate((element) => element.parentElement!.scrollWidth > element.parentElement!.clientWidth)).toBe(true);
          }
        }
        if (name === "record") {
          // The post's form has tabs; the category is under "Publishing".
          await page.getByRole("main").getByRole("tab", { name: "Publishing" }).click();
          await expect(page.getByRole("main").getByText("Guides", { exact: true })).toBeVisible();
        }
        await capture(page, name!, width, colorScheme);
        await fitsViewport(page);
      }
    });
  }
}

test("mobile navigation is a native modal with trapped focus, Escape and focus restoration", async ({ page, watch: _ }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  await signIn(page, "admin@example.test");
  const opener = page.getByRole("button", { name: "Open navigation" });
  await opener.click();
  const drawer = page.getByRole("dialog", { name: "Navigation", exact: true });
  await expect(drawer).toBeVisible();
  expect(await drawer.evaluate((element) => element.tagName === "DIALOG" && element.matches(":modal"))).toBe(true);
  await expect(opener).toHaveAttribute("aria-expanded", "true");
  await capture(page, "navigation", 390, "light");
  await page.emulateMedia({ colorScheme: "dark" });
  await capture(page, "navigation", 390, "dark");
  await opener.evaluate((element) => element.focus());
  expect(await drawer.evaluate((element) => element.contains(document.activeElement)), "the native modal makes background controls inert").toBe(true);
  const focusableCount = await drawer.locator("a, button").count();
  for (const key of ["Tab", "Shift+Tab"]) {
    for (let index = 0; index < focusableCount + 2; index++) {
      await page.keyboard.press(key);
      // Native Chrome dialogs can hand focus to browser chrome at a wrap (body
      // is then active). No actual focusable background element may receive it.
      expect(await drawer.evaluate((element) => element.contains(document.activeElement) || document.activeElement === document.body), "keyboard focus never reaches an inert background control").toBe(true);
    }
  }
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
  await expect(opener).toBeFocused();
  await expect(opener).toHaveAttribute("aria-expanded", "false");
  await opener.click();
  await drawer.getByRole("button", { name: "Close navigation" }).click();
  await expect(drawer).toBeHidden();
  await expect(opener).toBeFocused();
});

test("mobile navigation closes on links, including the current page and account", async ({ page, watch: _ }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page, "admin@example.test");
  const opener = page.getByRole("button", { name: "Open navigation" });
  const drawer = page.getByRole("dialog", { name: "Navigation", exact: true });
  for (let visit = 0; visit < 2; visit++) {
    await opener.click();
    await drawer.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Posts", exact: true }).click();
    await expect(page.getByRole("main").getByRole("heading", { name: "Posts", exact: true })).toBeVisible();
    await expect(drawer).toBeHidden();
  }
  await opener.click();
  await drawer.getByRole("link", { name: /Ada Admin/ }).click();
  await expect(page.getByRole("main").getByRole("heading", { name: "Your account" })).toBeVisible();
  await expect(drawer).toBeHidden();
});

test("crossing the tablet breakpoint closes the modal and restores usable desktop navigation", async ({ page, watch: _ }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page, "admin@example.test");
  await page.getByRole("button", { name: "Open navigation" }).click();
  const drawer = page.getByRole("dialog", { name: "Navigation", exact: true });
  await expect(drawer).toBeVisible();
  await page.setViewportSize({ width: 768, height: 900 });
  await expect(drawer).toBeHidden();
  expect(await page.locator("dialog").evaluate((element) => element.matches(":modal"))).toBe(false);
  await page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Posts", exact: true }).click();
  await expect(page.getByRole("main").getByRole("heading", { name: "Posts", exact: true })).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(drawer).toBeHidden();
  await page.getByRole("button", { name: "Open navigation" }).click();
  await expect(drawer).toBeVisible();
  await page.keyboard.press("Escape");
});

test("desktop sidebar collapses without losing accessible resource links", async ({ page, watch: _ }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page, "admin@example.test");
  const sidebar = page.locator("aside");
  const expanded = (await sidebar.boundingBox())!.width;
  await page.getByRole("button", { name: "Collapse sidebar" }).click();
  const expand = page.getByRole("button", { name: "Expand sidebar" });
  await expect(expand).toHaveAttribute("aria-expanded", "false");
  expect((await sidebar.boundingBox())!.width).toBeLessThan(expanded);
  await page.getByRole("navigation", { name: "Resources" }).getByRole("link", { name: "Posts", exact: true }).click();
  await expect(page.getByRole("main").getByRole("heading", { name: "Posts", exact: true })).toBeVisible();
  await expect(expand).toBeVisible();
  await fitsViewport(page);
  await expand.click();
  await expect(page.getByRole("button", { name: "Collapse sidebar" })).toHaveAttribute("aria-expanded", "true");
  expect((await sidebar.boundingBox())!.width).toBe(expanded);
});

test("clear filters preserves search, sorting and page size while resetting the page", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts?search=Post&sort=-title&per_page=10&page=2&filter%5Bpublished%5D%5Beq%5D=false");
  await expect(page.getByText("Page 2 of 2 · 15 records", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /Filters/, exact: true }).click();
  await expect(page.getByRole("combobox", { name: "Published" })).toBeHidden();
  await expect(page.getByText("Filtering by Published", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Clear filters", exact: true }).click();
  await expect(page.getByText("Page 1 of 3 · 30 records", { exact: true })).toBeVisible();
  const params = new URL(page.url()).searchParams;
  expect(params.get("search")).toBe("Post");
  expect(params.get("sort")).toBe("-title");
  expect(params.get("per_page")).toBe("10");
  expect(params.has("page")).toBe(false);
  expect([...params.keys()].some((key) => key.startsWith("filter["))).toBe(false);
  await expect(page.getByLabel("Search", { exact: true })).toHaveValue("Post");
  await expect(page.locator("thead th").filter({ has: page.getByRole("button", { name: /^Title(?: descending)?$/ }) })).toHaveAttribute("aria-sort", "descending");
  await expect(page.getByRole("combobox", { name: "Per page" })).toHaveValue("10");
  await expect(page.getByRole("button", { name: "Clear filters", exact: true })).toHaveCount(0);
});

test("select all exposes mixed state and selection resets with the shown rows", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts?per_page=10");
  await expect(page.getByText("Page 1 of 3 · 30 records", { exact: true })).toBeVisible();
  const selectAll = page.getByRole("checkbox", { name: "Select all", exact: true });
  const rowSelections = page.getByRole("table").locator("tbody").getByRole("checkbox");
  // Row checkboxes have record titles; no bulk action is run (seed remains unchanged).
  const first = page.getByRole("table").locator("tbody").getByRole("checkbox").first();
  await first.check();
  await expect(selectAll).toHaveAttribute("aria-checked", "mixed");
  expect(await selectAll.evaluate((element) => (element as HTMLInputElement).indeterminate)).toBe(true);
  await expect(page.getByText("1 selected record", { exact: true })).toBeVisible();
  await selectAll.check();
  await expect(selectAll).toBeChecked();
  expect(await selectAll.evaluate((element) => (element as HTMLInputElement).indeterminate)).toBe(false);
  await expect(page.getByText("10 selected records", { exact: true })).toBeVisible();
  await expect(rowSelections).toHaveCount(10);
  await expect(page.getByRole("table").locator("tbody input:checked")).toHaveCount(10);
  await page.getByRole("button", { name: "Next", exact: true }).click();
  await expect(page.getByText("Page 2 of 3 · 30 records", { exact: true })).toBeVisible();
  await expect(selectAll).not.toBeChecked();
  await expect(page.getByRole("table").locator("tbody input:checked")).toHaveCount(0);
  await expect(page.getByText(/selected records?/)).toHaveCount(0);
});

test("keyboard skip link focuses main without changing the current route or query", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts?search=Post");
  await expect(page.getByRole("heading", { name: "Posts", exact: true })).toBeVisible();
  const currentUrl = page.url();
  await page.keyboard.press("Tab");
  const skip = page.getByRole("link", { name: "Skip to content", exact: true });
  await expect(skip).toBeFocused();
  await expect(skip).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("main")).toBeFocused();
  await expect(page).toHaveURL(currentUrl);
});

test("long authorized metadata stays inside desktop, tablet and mobile layouts", async ({ page, watch: _ }) => {
  const label = "Extended authorized resource name ".repeat(5).trim();
  const name = "LongAdministratorName".repeat(6);
  await page.route("**/panel", async (route) => {
    const response = await route.fetch();
    expect(response.ok()).toBe(true);
    const panel = await response.json() as PanelResponse;
    // Preserve real authorization/navigation; only stress its display metadata.
    panel.brand = "LongWorkspaceBrand".repeat(8);
    panel.user.name = name;
    panel.user.email = `${"long.email.segment.".repeat(6)}@example.test`;
    panel.navigation = panel.navigation.map((resource) => ({ ...resource, pluralLabel: label }));
    await route.fulfill({ response, json: panel });
  });
  await signIn(page, "admin@example.test");
  for (const width of [1440, 768, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/admin/");
    await expect(page.getByRole("main").getByRole("heading", { name: `Welcome, ${name}`, exact: true })).toBeVisible();
    await fitsViewport(page);
    await capture(page, "long-metadata-home", width, "light");
    if (width < 768) {
      await page.getByRole("button", { name: "Open navigation" }).click();
      await expect(page.getByRole("dialog", { name: "Navigation" })).toBeVisible();
      await fitsViewport(page);
      await page.keyboard.press("Escape");
    }
    await page.goto("/admin/account");
    await expect(page.getByRole("heading", { name: "Your account", exact: true })).toBeVisible();
    await fitsViewport(page);
  }
});

test("record details are selectable label/value text, not disabled editing controls", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts/records/1");
  const main = page.getByRole("main");
  await expect(main.getByRole("heading", { name: "Post number 1", exact: true })).toBeVisible();
  for (const [label, value] of [["Title", "Post number 1"], ["Body", "Body of post 1."], ["Category", "Guides"], ["Published", "No"]]) {
    const field = main.locator("dl").filter({ has: page.locator("dt").filter({ hasText: new RegExp(`^${label}$`) }) });
    await expect(field.locator("dd")).toHaveText(value!);
  }
  // The details (relation sections below have their own search and selection).
  await expect(main.locator("dl").locator("input, textarea, select, [role='combobox'], [role='switch']")).toHaveCount(0);
  await expect(main.getByRole("link", { name: "Edit", exact: true })).toBeVisible();
});
