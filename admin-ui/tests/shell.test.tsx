import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";

const user = { id: "1", name: "Admin", email: "admin@example.test" };
const resource = { slug: "posts", label: "Post", pluralLabel: "Posts", icon: "file-text", group: "Content" };
const panel = { brand: "Workspace", user, navigation: [resource], system: { users: false, roles: false } };
const server = setupServer(
  http.get("*/api/auth/me", () => HttpResponse.json({ user, guard: "admin", permissions: [], superAdmin: false })),
  http.get("*/api/panel", () => HttpResponse.json(panel)),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function mount(basePath = "") {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath, apiPath: "http://localhost/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: [basePath + "/"] }));
  render(<SessionContext.Provider value={session}><QueryClientProvider client={queryClient}><RouterProvider router={router} /></QueryClientProvider></SessionContext.Provider>);
  return { router, queryClient };
}

it("uses authorized navigation for shortcuts and preserves the configured base path", async () => {
  mount("/control");
  expect(await screen.findByRole("heading", { name: "Welcome, Admin" })).toBeInTheDocument();
  const nav = screen.getByRole("navigation", { name: "Resources" });
  expect(within(nav).getByRole("link", { name: "Posts" })).toHaveAttribute("href", "/control/r/posts");
  expect(within(nav).queryByRole("link", { name: "Users" })).toBeNull();
  const main = screen.getByRole("main");
  expect(within(main).getByRole("link", { name: /Posts/ })).toHaveAttribute("href", "/control/r/posts");
  expect(within(main).queryByRole("link", { name: "Roles" })).toBeNull();
  expect(screen.getByRole("link", { name: "Skip to content" })).toHaveAttribute("href", "#main-content");
});

it("focuses main without following the document base URL when skipping content", async () => {
  const { router } = mount("/control");
  await screen.findByRole("heading", { name: "Welcome, Admin" });
  const before = router.state.location.href;
  expect(fireEvent.click(screen.getByRole("link", { name: "Skip to content" }))).toBe(false);
  expect(screen.getByRole("main")).toHaveFocus();
  expect(router.state.location.href).toBe(before);
});

it("keeps page content mounted across collapse and panel refresh", async () => {
  const { queryClient } = mount();
  const heading = await screen.findByRole("heading", { name: "Welcome, Admin" });
  await userEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }));
  expect(screen.getByRole("button", { name: "Expand sidebar" })).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByRole("heading", { name: "Welcome, Admin" })).toBe(heading);
  await act(() => queryClient.invalidateQueries({ queryKey: ["panel"] }));
  expect(screen.getByRole("heading", { name: "Welcome, Admin" })).toBe(heading);
});

it("opens a named native navigation dialog and closes without changing content", async () => {
  mount();
  const heading = await screen.findByRole("heading", { name: "Welcome, Admin" });
  await userEvent.click(screen.getByRole("button", { name: "Open navigation" }));
  const dialog = screen.getByRole("dialog", { name: "Navigation" });
  expect(within(dialog).getByRole("navigation", { name: "Resources" })).toBeInTheDocument();
  await userEvent.click(within(dialog).getByRole("button", { name: "Close navigation" }));
  expect(screen.queryByRole("dialog", { name: "Navigation" })).toBeNull();
  expect(screen.getByRole("heading", { name: "Welcome, Admin" })).toBe(heading);
});

it("shows a helpful empty state without inventing resources", async () => {
  server.use(http.get("*/api/panel", () => HttpResponse.json({ ...panel, navigation: [] })));
  mount();
  expect(await screen.findByText(/There is nothing you can manage yet/)).toBeInTheDocument();
  expect(within(screen.getByRole("main")).queryByRole("link", { name: /Posts/ })).toBeNull();
});
