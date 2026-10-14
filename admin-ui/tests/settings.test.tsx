import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode } from "react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { SettingsResponse } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { setLocale } from "@/i18n";
import fixture from "../../fixtures/contract/settings.json";

const settings = fixture as unknown as SettingsResponse;
let saved: { version: string; data: Record<string, unknown> }[] = [];
let answer: () => Response = () => HttpResponse.json({ ...settings, version: "1" });

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({
      brand: "T",
      user: { id: "1", name: "A", email: "a@x.y" },
      navigation: [],
      system: { users: false, roles: false },
      settings: [{ group: "general", label: "General", icon: "settings" }],
    }),
  ),
  http.get("*/admin/api/settings/general", () => HttpResponse.json(settings)),
  http.put("*/admin/api/settings/general", async ({ request }) => {
    saved.push((await request.json()) as (typeof saved)[number]);
    return answer();
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => {
  server.resetHandlers();
  saved = [];
  answer = () => HttpResponse.json({ ...settings, version: "1" });
});

function mount(strict = false) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/settings/general"] }));
  const app = (
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>
  );
  render(strict ? <StrictMode>{app}</StrictMode> : app);
}

const secretBlock = (label: string) => screen.getByText(label).closest("div")!;

it("renders every input from the contract, and links the page", async () => {
  mount();
  expect(await screen.findByLabelText(/^Site name/)).toHaveValue("Academy");
  expect(document.querySelector("[data-fallback]")).toBeNull();
  expect(within(secretBlock("SMTP password")).getByText("Not set")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "General" })).toHaveAttribute("href", "/settings/general");
});

it("saves only what changed, with the version, and never the secrets left alone", async () => {
  mount();
  const name = await screen.findByLabelText(/^Site name/);
  await userEvent.clear(name);
  await userEvent.type(name, "LMS");
  await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
  await screen.findByText("Settings saved.");
  expect(saved).toEqual([{ version: "0", data: { site_name: "LMS" } }]);
});

it("replaces, clears and undoes secrets explicitly", async () => {
  server.use(http.get("*/admin/api/settings/general", () => HttpResponse.json({ ...settings, secrets: { smtp_password: "set", api_key: "set" } })));
  mount();
  await screen.findByLabelText(/^Site name/);
  const smtp = secretBlock("SMTP password");
  await userEvent.click(within(smtp).getByRole("button", { name: "Replace" }));
  await userEvent.type(screen.getByLabelText("SMTP password"), "new-secret");
  const api = secretBlock("Api key");
  await userEvent.click(within(api).getByRole("button", { name: "Clear" }));
  expect(within(api).getByText("Will be cleared when you save.")).toBeInTheDocument();
  await userEvent.click(within(api).getByRole("button", { name: "Undo" }));
  await userEvent.click(within(api).getByRole("button", { name: "Clear" }));
  await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
  await screen.findByText("Settings saved.");
  expect(saved[0]!.data).toEqual({ smtp_password: "new-secret", api_key: null });
});

it("asks for a value instead of sending an empty secret", async () => {
  mount();
  await screen.findByLabelText(/^Site name/);
  await userEvent.click(within(secretBlock("SMTP password")).getByRole("button", { name: "Set a value" }));
  await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
  expect(await screen.findByText("Enter a value, or choose Clear to remove it.")).toBeInTheDocument();
  expect(saved).toEqual([]);
});

it("keeps the draft when someone else saved meanwhile", async () => {
  answer = () =>
    HttpResponse.json(
      {
        message: "The given data was invalid.",
        code: "validation_failed",
        errors: {},
        formErrors: ["This was changed by someone else meanwhile. Reload and try again."],
        issues: {},
        formIssues: [{ code: "stale", params: {} }],
      },
      { status: 422 },
    );
  mount();
  await screen.findByLabelText(/^Site name/);
  await userEvent.click(within(secretBlock("SMTP password")).getByRole("button", { name: "Set a value" }));
  await userEvent.type(screen.getByLabelText("SMTP password"), "draft");
  await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
  expect(await screen.findByText(/Someone else saved these settings meanwhile/)).toBeInTheDocument();
  expect(screen.getByLabelText("SMTP password")).toHaveValue("draft");
  expect(screen.getByRole("button", { name: "Save settings" })).toBeDisabled();
});

it("shows the settings read-only without the update ability", async () => {
  const readOnly = structuredClone(settings);
  readOnly.abilities.update = false;
  server.use(http.get("*/admin/api/settings/general", () => HttpResponse.json(readOnly)));
  mount();
  expect(await screen.findByLabelText(/^Site name/)).toBeDisabled();
  expect(screen.queryByRole("button", { name: "Save settings" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Set a value" })).toBeNull();
  await waitFor(() => expect(screen.getByText("You may view these settings but not change them.")).toBeInTheDocument());
});

it("completes saves under StrictMode", async () => {
  mount(true);
  await screen.findByLabelText(/^Site name/);
  await userEvent.click(within(secretBlock("SMTP password")).getByRole("button", { name: "Set a value" }));
  await userEvent.type(screen.getByLabelText("SMTP password"), "s3cret");
  await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
  expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
  // The intent was consumed.
  expect(screen.queryByLabelText("SMTP password")).toBeNull();
});

it("starts over from the current version after a stale save", async () => {
  let version = "0";
  server.use(http.get("*/admin/api/settings/general", () => HttpResponse.json({ ...settings, version })));
  answer = () => {
    if (saved.length === 1) {
      version = "1";
      return HttpResponse.json(
        { message: "x", code: "validation_failed", errors: {}, formErrors: ["stale"], issues: {}, formIssues: [{ code: "stale", params: {} }] },
        { status: 422 },
      );
    }
    return HttpResponse.json({ ...settings, version: "2" });
  };
  mount();
  const name = await screen.findByLabelText(/^Site name/);
  await userEvent.clear(name);
  await userEvent.type(name, "Mine");
  await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
  await userEvent.click(await screen.findByRole("button", { name: "Discard my changes" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Save settings" })).toBeEnabled());
  const again = screen.getByLabelText(/^Site name/);
  await userEvent.clear(again);
  await userEvent.type(again, "Mine");
  await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
  await screen.findByText("Settings saved.");
  expect(saved.map((s) => s.version)).toEqual(["0", "1"]);
});

it("keeps the draft when the page cannot be refetched in another language", async () => {
  mount();
  await screen.findByLabelText(/^Site name/);
  await userEvent.click(within(secretBlock("SMTP password")).getByRole("button", { name: "Set a value" }));
  await userEvent.type(screen.getByLabelText("SMTP password"), "draft");
  let failed = 0;
  server.use(http.get("*/admin/api/settings/general", () => {
    failed += 1;
    return HttpResponse.json({ message: "down", code: "internal_error" }, { status: 500 });
  }));
  try {
    act(() => setLocale("vi"));
    await waitFor(() => expect(failed).toBeGreaterThan(0));
    await waitFor(() => expect(screen.getByText(/Không làm mới được trang/)).toBeInTheDocument());
    expect(screen.getByDisplayValue("draft")).toBeInTheDocument();
  } finally {
    act(() => setLocale("en"));
  }
});

it("follows a refetched loss of the update ability", async () => {
  const client = { reads: 0 };
  server.use(
    http.get("*/admin/api/settings/general", () => {
      client.reads += 1;
      const answer = structuredClone(settings);
      answer.abilities.update = client.reads === 1;
      return HttpResponse.json(answer);
    }),
  );
  mount();
  await screen.findByRole("button", { name: "Save settings" });
  try {
    act(() => setLocale("vi"));
    await waitFor(() => expect(screen.queryByRole("button", { name: /Lưu cài đặt|Save settings/ })).toBeNull());
  } finally {
    act(() => setLocale("en"));
  }
});
