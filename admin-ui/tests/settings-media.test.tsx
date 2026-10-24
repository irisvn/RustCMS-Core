import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { SettingsResponse } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import fixture from "../../fixtures/contract/settings.brand.json";

const settings = fixture as unknown as SettingsResponse;
const ID = "0199d3a4-1b2c-7d3e-8f40-123456789abc";
let saved: { version: string; data: Record<string, unknown> }[] = [];

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@x.y" }, navigation: [], settings: [{ group: "brand", label: "Brand" }] }),
  ),
  http.get("*/admin/api/settings/brand", () => HttpResponse.json(settings)),
  http.put("*/admin/api/settings/brand", async ({ request }) => {
    const body = (await request.json()) as (typeof saved)[number];
    saved.push(body);
    return HttpResponse.json({ ...settings, version: "1", data: { ...settings.data, ...body.data } });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => {
  saved = [];
  vi.unstubAllGlobals();
});

function mount() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/settings/brand"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
}

it("edits a media setting with the media input, and saves its id once the upload is done", async () => {
  let release: () => void = () => {};
  // Node's fetch cannot send jsdom's FormData: answer at the fetch boundary.
  const real = globalThis.fetch;
  vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
    if (!url.endsWith("/media")) return real(url, init);
    await new Promise<void>((resolve) => (release = resolve));
    const data = { id: ID, name: "logo.png", mime: "image/png", size: 3, inline: true };
    return new Response(JSON.stringify({ data }), { status: 201, headers: { "Content-Type": "application/json" } });
  });
  mount();
  const logo = await screen.findByRole("group", { name: "Logo" });
  expect(within(logo).getByText("No file.")).toBeInTheDocument();
  expect(screen.getByRole("group", { name: "Slides" })).toBeInTheDocument();
  await userEvent.upload(logo.querySelector("input[type=file]") as HTMLInputElement, new File(["png"], "logo.png", { type: "image/png" }));
  // Not saved while the file is on its way.
  await waitFor(() => expect(screen.getByRole("button", { name: "Save settings" })).toBeDisabled());
  release();
  await waitFor(() => expect(screen.getByRole("button", { name: "Save settings" })).toBeEnabled());
  await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
  await waitFor(() => expect(saved).toEqual([{ version: "0", data: { logo: ID } }]));
});
