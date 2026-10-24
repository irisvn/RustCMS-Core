import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { ResourceSchema } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import posts from "../../fixtures/contract/posts.schema.json";

// Categories (parent) have posts (children, with actions) through `category_id`.
const child = posts as unknown as ResourceSchema;
const parent: ResourceSchema = {
  ...child,
  resource: { ...child.resource, slug: "categories", label: "Category", pluralLabel: "Categories" },
  operation: "view",
  abilities: { view: true, viewAny: true },
  table: { ...child.table, actions: [] },
  relations: [{ name: "posts", kind: "hasMany", label: "Posts", resource: "posts", field: "category_id", create: false }],
};
const post = (key: string, abilities: Record<string, boolean>) => ({ key, title: `Post ${key}`, abilities: { view: true, ...abilities }, data: { title: `Post ${key}`, category_id: 1 } });
const runs: { action: string; body: unknown }[] = [];

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1", name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [parent.resource, child.resource] })),
  http.get("*/admin/api/resources/categories/schema", () => HttpResponse.json(parent)),
  http.get("*/admin/api/resources/posts/schema", ({ request }) => HttpResponse.json({ ...child, operation: new URL(request.url).searchParams.get("operation") })),
  http.get("*/admin/api/resources/categories/records/1", () => HttpResponse.json({ data: { key: "1", title: "News", abilities: { view: true }, data: { title: "News" } } })),
  http.get("*/admin/api/resources/posts/records/a", () => HttpResponse.json({ data: post("a", { touch: true, archive: true, unpublish: true }) })),
  http.get("*/admin/api/resources/categories/records/1/relations/posts", () =>
    HttpResponse.json({ data: [post("a", { touch: true }), post("b", { archive: true })], meta: { page: 1, perPage: 10, total: 2, lastPage: 1 } })),
  http.post("*/admin/api/resources/posts/actions/:action", async ({ request, params }) => {
    runs.push({ action: String(params.action), body: await request.json() });
    return HttpResponse.json({ processed: ["a"], denied: [], missing: [], message: "Done here." });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  runs.length = 0;
});

function mount(path: string): ReturnType<typeof createAppRouter> {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: [path] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  return router;
}

it("shows a record's actions on its page: the primary one labelled, the others under ⋯", async () => {
  mount("/r/posts/records/a");
  const touch = await screen.findByRole("button", { name: "Touch" });
  await userEvent.click(screen.getByRole("button", { name: "More actions for Post a" }));
  const menu = await screen.findByRole("menu");
  expect([...menu.children].map((c) => c.getAttribute("role") === "separator" ? "---" : c.textContent)).toEqual(["Archive", "---", "Unpublish"]);
  await userEvent.keyboard("{Escape}");
  await userEvent.click(touch);
  await waitFor(() => expect(runs).toEqual([{ action: "touch", body: { keys: ["a"], data: {} } }]));
  expect(await screen.findByText(/^Done here\./)).toBeInTheDocument();
});

it("shows the related resource's row actions in relation tables", async () => {
  mount("/r/categories/records/1");
  const table = await screen.findByRole("table", { name: "Posts" });
  const rowA = within(table).getByText("Post a").closest("tr")!;
  const rowB = within(table).getByText("Post b").closest("tr")!;
  expect(within(rowA).queryByRole("button", { name: /More actions/ })).toBeNull();
  await userEvent.click(within(rowA).getByRole("button", { name: "Touch" }));
  await waitFor(() => expect(runs).toEqual([{ action: "touch", body: { keys: ["a"], data: {} } }]));
  // Offered per row: `b` only offers Archive (in its menu).
  expect(within(rowB).queryByRole("button", { name: "Touch" })).toBeNull();
  await userEvent.click(within(rowB).getByRole("button", { name: "More actions for Post b" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "Archive" }));
  await waitFor(() => expect(runs[1]).toEqual({ action: "archive", body: { keys: ["b"], data: {} } }));
});

it("never opens a page for a run whose page was left before it answered", async () => {
  const opened: unknown[] = [];
  vi.stubGlobal("open", (...args: unknown[]) => opened.push(args));
  onTestFinished(() => {
    vi.unstubAllGlobals();
  });
  let answer: () => void = () => {};
  server.use(http.post("*/admin/api/resources/posts/actions/:action", async () => {
    await new Promise<void>((resolve) => (answer = resolve));
    return HttpResponse.json({ processed: ["a"], denied: [], missing: [], open: "https://example.com/late" });
  }));
  const router = mount("/r/posts/records/a");
  await userEvent.click(await screen.findByRole("button", { name: "Touch" }));
  await act(() => router.navigate({ to: "/r/$slug/records/$key", params: { slug: "categories", key: "1" } }));
  answer();
  await new Promise((resolve) => setTimeout(resolve, 50));
  expect(opened).toEqual([]);
  server.resetHandlers();
});

it("offers a safe retry for a related record's action whose outcome is unknown", async () => {
  let attempts = 0;
  server.use(http.post("*/admin/api/resources/posts/actions/:action", async ({ request }) => {
    attempts += 1;
    runs.push({ action: "touch", body: await request.json() });
    return attempts === 1 ? new HttpResponse(null, { status: 504 }) : HttpResponse.json({ processed: ["a"], denied: [], missing: [], message: "Touched.", replayed: true });
  }));
  mount("/r/categories/records/1");
  const table = await screen.findByRole("table", { name: "Posts" });
  await userEvent.click(within(within(table).getByText("Post a").closest("tr")!).getByRole("button", { name: "Touch" }));
  // The related resource is paused: its banner is on this section.
  await userEvent.click(await screen.findByRole("button", { name: "Retry safely" }));
  expect(await screen.findByText(/^Touched\./)).toBeInTheDocument();
  expect(attempts).toBe(2);
  server.resetHandlers();
});

it("does not run an action on a related record that left the table while its dialog was open", async () => {
  let rows = [post("a", { touch: true }), post("b", { archive: true }), post("c", { publish: true })];
  server.use(http.get("*/admin/api/resources/categories/records/1/relations/posts", () =>
    HttpResponse.json({ data: rows, meta: { page: 1, perPage: 10, total: rows.length, lastPage: 1 } })));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/r/categories/records/1"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  const table = await screen.findByRole("table", { name: "Posts" });
  await userEvent.click(within(within(table).getByText("Post c").closest("tr")!).getByRole("button", { name: "More actions for Post c" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "Publish" }));
  // Moved to another category meanwhile: the refetched rows no longer have it.
  rows = [post("a", { touch: true })];
  await act(() => queryClient.invalidateQueries());
  await waitFor(() => expect(within(table).queryByText("Post c")).toBeNull());
  await userEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Confirm" }));
  expect(await screen.findByText(/changed meanwhile|was changed/i)).toBeInTheDocument();
  expect(runs).toEqual([]);
  server.resetHandlers();
});
