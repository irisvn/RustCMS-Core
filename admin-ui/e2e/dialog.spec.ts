import { expect, signIn, test } from "./fixtures";

// The dialog's close button (0.7.1), in a real browser (native <dialog>).
test("a dialog opens on its first control, closes with Escape or ×, and gives focus back", async ({ page, watch: _ }) => {
  await signIn(page, "admin@example.test");
  await page.goto("/admin/system/roles");
  await page.getByRole("link", { name: "editor", exact: true }).click();
  const open = page.getByRole("button", { name: "Delete role" });
  await open.click();
  const dialog = page.getByRole("dialog", { name: "Delete role?" });
  await expect(dialog).toBeVisible();
  // The first control of the content, not the × placed after it.
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(open).toBeFocused();
  await open.click();
  await dialog.getByRole("button", { name: "Close" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("heading", { name: "editor" })).toBeVisible();
});
