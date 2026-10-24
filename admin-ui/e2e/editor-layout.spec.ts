import { expect, signIn, test } from "./fixtures";

// The demo's category form (vocabulary 11): the whole width, a main column
// with a prefixed slug, and an aside with a compact section that folds.
// Nothing is saved.
test("an editor layout: full width and a sticky aside at 1440px, one column at 390px", async ({ page, watch: _ }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/categories/create");
  const main = page.getByRole("main");
  const name = page.getByLabel("Name");
  const aside = main.locator("aside");
  await expect(name).toBeVisible();

  // Wider than the default form (56rem), the aside about 20rem on the right, sticky.
  const form = await main.locator("form").boundingBox();
  expect(form!.width).toBeGreaterThan(1000);
  const nameBox = (await name.boundingBox())!;
  const asideBox = (await aside.boundingBox())!;
  expect(asideBox.x).toBeGreaterThan(nameBox.x + nameBox.width);
  expect(Math.round(asideBox.width)).toBe(320);
  expect(await aside.evaluate((el) => getComputedStyle(el).position)).toBe("sticky");

  // The slug's prefix: inside the input's border, not in its value, read with it.
  const slug = page.getByLabel("Slug");
  await expect(main.getByText("/categories/", { exact: true })).toBeVisible();
  await slug.fill("tools");
  await expect(slug).toHaveValue("tools");
  const described = (await slug.getAttribute("aria-describedby")) ?? "";
  expect(await page.locator(`[id="${described.split(" ")[0]}"]`).textContent()).toBe("/categories/");

  // The aside's section folds and unfolds.
  const display = aside.getByRole("button", { name: "Display" });
  await expect(display).toHaveAttribute("aria-expanded", "true");
  await expect(aside.getByRole("group", { name: "Color" })).toBeVisible();
  await display.click();
  await expect(display).toHaveAttribute("aria-expanded", "false");
  await expect(aside.getByRole("group", { name: "Color" })).toBeHidden();
  await display.click();
  await expect(aside.getByRole("group", { name: "Color" })).toBeVisible();

  // Narrow: one column, the aside below the main column, nothing wider than the screen.
  await page.setViewportSize({ width: 390, height: 844 });
  const narrowName = (await name.boundingBox())!;
  const narrowAside = (await aside.boundingBox())!;
  expect(narrowAside.y).toBeGreaterThan(narrowName.y + narrowName.height);
  expect(Math.abs(narrowAside.x - narrowName.x)).toBeLessThan(40);
  expect(await aside.evaluate((el) => getComputedStyle(el).position)).not.toBe("sticky");
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  expect(await page.locator("[style]").count(), "no inline styles").toBe(0);
});

// A record page on a phone: the primary action and ⋯ (the others, Edit and
// Delete included, Delete last), the title keeping its width; inline on a
// wide screen.
test("a record page's actions on a phone: the primary one and ⋯, inline on a wide screen", async ({ page, watch: _ }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/posts/records/3");
  const title = page.getByRole("heading", { level: 1 });
  await expect(title).toBeVisible();
  await expect(page.getByRole("link", { name: "Edit" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Delete", exact: true })).toHaveCount(0);
  const more = page.getByRole("button", { name: /^More actions for/ });
  await expect(more).toBeVisible();
  expect((await title.boundingBox())!.width).toBeGreaterThanOrEqual(250);
  await more.click();
  const menu = page.getByRole("menu");
  const items = await menu.locator(":scope > *").evaluateAll((list) =>
    list.map((el) => (el.getAttribute("role") === "separator" ? "---" : (el.textContent ?? "").trim())));
  expect(items).toContain("Edit");
  expect(items.at(-1)).toBe("Delete");
  expect(items.at(-2)).toBe("---");
  await menu.getByRole("menuitem", { name: "Edit" }).click();
  await expect(page).toHaveURL(/\/admin\/r\/posts\/records\/3\/edit$/);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);

  // Wide: every action inline again.
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/admin/r/posts/records/3");
  await expect(page.getByRole("link", { name: "Edit" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Delete", exact: true })).toBeVisible();
});
