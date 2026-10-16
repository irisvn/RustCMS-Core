// Invitations, password resets (mails read from Mailpit), remember me, and
// the dashboard's widgets — without a CSP violation.
import { type Page } from "@playwright/test";
import { PASSWORD, expect, signIn, test } from "./fixtures";

const MAILPIT = process.env.RUSTCMS_MAILPIT_API ?? "http://127.0.0.1:58025";

/** The link in the newest mail to `to` whose subject matches, waiting for it. */
async function linkIn(page: Page, to: string, subject: RegExp, page_: string): Promise<string> {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    // Node's own fetch: Mailpit is outside the app under test.
    const found = await fetch(`${MAILPIT}/api/v1/search?query=${encodeURIComponent(`to:${to}`)}`);
    const list = (await found.json()) as { messages: { ID: string; Subject: string }[] };
    const mail = list.messages.find((m) => subject.test(m.Subject));
    if (mail) {
      const message = (await (await fetch(`${MAILPIT}/api/v1/message/${mail.ID}`)).json()) as { Text: string };
      const match = message.Text.match(new RegExp(`(http\\S+/admin/${page_}#token=\\S+)`));
      if (match?.[1]) return match[1];
    }
    await page.waitForTimeout(500);
  }
  throw new Error(`no mail to ${to}`);
}

test("an invited user chooses a password, resets it, and is remembered", async ({ page, watch }) => {
  const email = `invited-${Date.now()}@demo.test`;
  await signIn(page, "admin@example.test");
  await page.goto("/admin/system/users/create");
  await page.getByLabel("Name").fill("Invited Person");
  await page.getByLabel("Email").fill(email);
  await page.getByLabel(/Send an invitation instead/).check();
  await page.getByRole("button", { name: "Send the invitation" }).click();
  // On the new user's page: their invitation, pending.
  await expect(page.getByRole("heading", { name: "Invitation" })).toBeVisible();
  await page.context().clearCookies();

  // The invitation: the token comes in the fragment and leaves the address bar.
  const invitation = await linkIn(page, email, /invited/i, "invitation");
  await page.goto(invitation);
  await expect(page).toHaveURL(/\/admin\/invitation$/);
  await page.getByLabel("New password").fill("a brand new passphrase");
  await page.getByLabel("Confirm the password").fill("a brand new passphrase");
  await page.getByRole("button", { name: "Accept the invitation" }).click();
  await expect(page.getByText("Your account is ready")).toBeVisible();

  // Forgot it: the same answer, a reset link, a new password.
  await page.getByRole("link", { name: "Forgot password?" }).click();
  await page.getByLabel("Email").fill(email);
  await page.getByRole("button", { name: "Send the link" }).click();
  await expect(page.getByText(/If this email has an account/)).toBeVisible();
  const reset = await linkIn(page, email, /reset your password/i, "reset-password");
  await page.goto(reset);
  await expect(page).toHaveURL(/\/admin\/reset-password$/);
  await page.getByLabel("New password").fill("another passphrase here");
  await page.getByLabel("Confirm the password").fill("another passphrase here");
  await page.getByRole("button", { name: "Save the password" }).click();
  await expect(page.getByText("Your password was changed")).toBeVisible();

  // Remember me: a cookie that outlives the browser.
  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password").fill("another passphrase here");
  await page.getByLabel("Remember me").check();
  await page.getByRole("button", { name: /^Sign in/ }).click();
  await expect(page.getByRole("main").getByRole("heading", { name: /^Welcome/ })).toBeVisible();
  const session = (await page.context().cookies()).find((c) => c.name === "rustcms_session");
  expect(session?.expires ?? 0).toBeGreaterThan(Date.now() / 1000 + 29 * 86_400);
  void watch;
  void PASSWORD;
});

test("the dashboard shows its widgets", async ({ page, watch }) => {
  await signIn(page, "admin@example.test");
  await expect(page.locator("h2", { hasText: /^Posts$/ })).toBeVisible();
  await expect(page.getByRole("img", { name: "Published per week" })).toBeVisible();
  await expect(page.getByRole("img", { name: "Published per week" }).locator("rect")).toHaveCount(8);
  void watch;
});
