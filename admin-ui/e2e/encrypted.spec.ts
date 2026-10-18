import { expect, signIn, test } from "./fixtures";

// The demo's Integrations (`Field::encrypted`, `Field::encrypted_map`): secrets
// are set, kept, replaced and removed, and never reach the browser — not in
// the DOM, not in any response.
test("encrypted fields are written and never shown", async ({ page, watch: _ }) => {
  const SECRET = "e2e-secret-7f3a";
  const bodies: string[] = [];
  page.on("response", async (response) => {
    if (response.url().includes("/admin/api/")) bodies.push(await response.text().catch(() => ""));
  });
  await signIn(page, "admin@example.test");
  await page.goto("/admin/r/integrations/create");
  await page.getByLabel("Name").fill("Billing");
  await page.getByRole("button", { name: "Set a value" }).click();
  await page.getByPlaceholder("New value").first().fill(`${SECRET}-token`);
  await page.getByRole("button", { name: "Add entry" }).click();
  await page.getByLabel("Key 1").fill("DB_PASSWORD");
  await page.getByLabel("Value 1").fill(`${SECRET}-db`);
  await page.getByRole("button", { name: "Add entry" }).click();
  await page.getByLabel("Key 2").fill("API_URL");
  await page.getByLabel("Value 2").fill(`${SECRET}-url`);
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: "Billing" })).toBeVisible();
  const record = page.url().replace(/\/$/, "");

  // The edit form: the names only, each kept.
  await page.goto(`${record}/edit`);
  const env = page.getByRole("group", { name: "Environment" });
  await expect(env.getByText("DB_PASSWORD")).toBeVisible();
  await expect(env.getByText("API_URL")).toBeVisible();
  await expect(env.getByText("Kept")).toHaveCount(2);
  expect(await page.content()).not.toContain(SECRET);

  // Replace one, remove the other, save.
  await env.getByRole("button", { name: "Replace" }).first().click();
  await env.getByLabel(/^Value/).fill(`${SECRET}-new`);
  await env.getByRole("button", { name: /Remove entry/ }).last().click();
  const saved = page.waitForResponse((r) => r.request().method() === "PATCH" && r.url().includes("/integrations/records/"));
  await page.getByRole("button", { name: "Save" }).click();
  expect((await saved).status()).toBe(200);
  await page.goto(`${record}/edit`);
  const after = page.getByRole("group", { name: "Environment" });
  await expect(after.getByText("Kept")).toHaveCount(1);
  expect(await page.content()).not.toContain(SECRET);
  for (const body of bodies) {
    expect(body).not.toContain(SECRET);
    expect(body).not.toContain("rcms1.");
  }
});
