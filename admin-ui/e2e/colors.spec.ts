import { type Locator, type Page } from "@playwright/test";
import { expect, signIn, srgb, test } from "./fixtures";

// Components take their colors from the tokens in both schemes: checkboxes
// drawn by the panel, solid and soft badges, disabled buttons that change
// color (not opacity). The tokens' own contrast is checked in layout.spec.ts.

const rgb = async (page: Page, color: string) => (await srgb(page, [color]))[0]!;
const token = (page: Page, name: string) => rgb(page, `--${name}`);

const style = async (locator: Locator, property: "color" | "backgroundColor" | "borderTopColor" | "opacity") =>
  locator.evaluate((element, p) => getComputedStyle(element)[p], property);

for (const colorScheme of ["light", "dark"] as const) {
  test(`components follow the color tokens in ${colorScheme} mode`, async ({ page, watch: _ }) => {
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await signIn(page, "admin@example.test");

    // Badges: primary solid, info and gray soft.
    await page.goto("/admin/r/tags");
    const series = page.getByRole("row", { name: /Security/ }).getByText("Series", { exact: true });
    const topic = page.getByRole("row", { name: /Rust/ }).getByText("Topic", { exact: true });
    await expect(series).toBeVisible();
    expect(await rgb(page, await style(series, "backgroundColor"))).toEqual(await token(page, "primary"));
    expect(await rgb(page, await style(series, "color"))).toEqual(await token(page, "primary-foreground"));
    expect(await rgb(page, await style(topic, "backgroundColor"))).toEqual(await token(page, "info-soft"));
    expect(await rgb(page, await style(topic, "color"))).toEqual(await token(page, "info-soft-foreground"));

    // A disabled button changes color, not opacity.
    const previous = page.getByRole("button", { name: "Previous" });
    await expect(previous).toBeDisabled();
    expect(await style(previous, "opacity")).toBe("1");
    expect(await rgb(page, await style(previous, "color"))).toEqual(await token(page, "subtle-foreground"));
    const create = page.getByRole("link", { name: "New tag" }).or(page.getByRole("button", { name: "New tag" })).first();
    expect(await rgb(page, await style(create, "color"))).not.toEqual(await token(page, "subtle-foreground"));

    // Checkboxes: the token border, primary once checked (or partly).
    await page.goto("/admin/r/posts");
    const boxes = page.getByRole("main").getByRole("checkbox");
    const row = boxes.nth(1);
    await expect(row).toBeVisible();
    expect(await rgb(page, await style(row, "borderTopColor"))).toEqual(await token(page, "input"));
    await row.check();
    const primary = await token(page, "primary");
    await expect.poll(async () => rgb(page, await style(row, "backgroundColor"))).toEqual(primary);
    const all = boxes.first();
    await expect(all).toHaveAttribute("aria-checked", "mixed");
    await expect.poll(async () => rgb(page, await style(all, "backgroundColor"))).toEqual(primary);
    // The selected row stands apart from the others.
    const selected = page.getByRole("main").locator("tr[data-selected]");
    await expect(selected).toHaveCount(1);
    await expect.poll(async () => rgb(page, await style(selected, "backgroundColor"))).toEqual(await token(page, "selected"));
    await row.uncheck();
  });
}
