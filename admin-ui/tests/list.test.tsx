import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { ResourceSchema } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import posts from "../../fixtures/contract/posts.schema.json";

const schema = posts as unknown as ResourceSchema;
const row = (key: string) => ({ key, title: `Post ${key}`, abilities: { view: true, update: true, delete: true, publish: true }, data: { title: `Post ${key}` } });
let releasePage2: () => void = () => {};
const actions: unknown[] = [];

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1", name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [schema.resource] })),
  http.get("*/admin/api/resources/posts/schema", () => HttpResponse.json({ ...schema, operation: "view" })),
  http.get("*/admin/api/resources/posts/records", async ({ request }) => {
    const page = new URL(request.url).searchParams.get("page") ?? "1";
    if (page === "2") await new Promise<void>((resolve) => (releasePage2 = resolve));
    const keys = page === "1" ? ["a", "b"] : ["c", "d"];
    return HttpResponse.json({ data: keys.map(row), meta: { page: Number(page), perPage: 2, total: 4, lastPage: 2 } });
  }),
  http.post("*/admin/api/resources/posts/actions/:action", async ({ request }) => {
    actions.push(await request.json());
    return HttpResponse.json({ processed: [], denied: [], missing: [] });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());

let router: ReturnType<typeof createAppRouter>;

function mount(search = "") {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/r/posts" + search] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
}

it("shows mixed selection without changing row ownership", async () => {
  mount();
  await userEvent.click(await screen.findByLabelText("Select Post a"));
  expect(screen.getByLabelText("Select all")).toBePartiallyChecked();
  await userEvent.click(screen.getByLabelText("Select all"));
  expect(screen.getByLabelText("Select all")).toBeChecked();
  expect(screen.getByLabelText("Select Post b")).toBeChecked();
});

it("clears only filters and keeps search, sort and page size", async () => {
  mount("?search=Post&sort=-title&per_page=25&filter[published][op]=eq&filter[published][value]=true");
  const clear = await screen.findByRole("button", { name: "Clear filters" });
  await userEvent.click(clear);
  await waitFor(() => expect(router.state.location.search).toEqual({ search: "Post", sort: "-title", per_page: "25" }));
  expect(screen.getByRole("columnheader", { name: /Title/ })).toHaveAttribute("aria-sort", "descending");
});

it("pauses actions after one with an unknown outcome until a safe retry", async () => {
  const keys: (string | null)[] = [];
  let outcome = 504;
  server.use(http.post("*/admin/api/resources/posts/actions/:action", async ({ request }) => {
    keys.push(request.headers.get("Idempotency-Key"));
    actions.push(await request.json());
    return outcome === 504 ? new HttpResponse(null, { status: 504 }) : HttpResponse.json({ processed: ["a"], denied: [], missing: [], replayed: true });
  }));
  try {
    mount();
    await userEvent.click(await screen.findByLabelText("Select Post a"));
    await userEvent.click(screen.getByRole("button", { name: "Publish" }));
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(await screen.findByText(/may not have been saved/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Publish" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
    // Leaving and coming back does not lift the pause.
    await act(() => router.navigate({ to: "/" }));
    await act(() => router.navigate({ to: "/r/$slug", params: { slug: "posts" } }));
    expect(await screen.findByText(/may not have been saved/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /New post/ })).toBeNull();

    // Still unknown: stays paused.
    await userEvent.click(screen.getByRole("button", { name: "Retry safely" }));
    await waitFor(() => expect(keys).toHaveLength(2));
    expect(await screen.findByText(/may not have been saved/)).toBeInTheDocument();
    // Applied (or replayed): the pause lifts. Every attempt carried the same key and body.
    outcome = 200;
    await userEvent.click(screen.getByRole("button", { name: "Retry safely" }));
    await waitFor(() => expect(screen.queryByText(/may not have been saved/)).toBeNull());
    expect(new Set(keys).size).toBe(1);
    expect(keys[0]).toMatch(/^[0-9a-f-]{36}$/);
    expect(actions.slice(-3)).toEqual([{ keys: ["a"], data: {} }, { keys: ["a"], data: {} }, { keys: ["a"], data: {} }]);
    expect(await screen.findByRole("button", { name: /New post/ })).toBeInTheDocument();
  } finally {
    server.resetHandlers();
  }
});

it("never acts on rows of the previous page while the next one loads", async () => {
  actions.length = 0;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/r/posts"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  expect(await screen.findByText("Post a")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Next" }));
  // Page 1 rows stay visible while page 2 loads, but cannot be selected.
  await waitFor(() => expect(screen.getByLabelText("Select all")).toBeDisabled());
  expect(screen.getByLabelText("Select Post a")).toBeDisabled();
  releasePage2();
  expect(await screen.findByText("Post c")).toBeInTheDocument();
  expect(screen.getByLabelText("Select all")).not.toBeChecked();
  await userEvent.click(screen.getByLabelText("Select Post c"));
  await userEvent.click(screen.getByRole("button", { name: "Publish" }));
  await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(actions).toEqual([{ keys: ["c"], data: {} }]));
});

it("follows the URL search on back and forward", async () => {
  mount("?search=first");
  const box = await screen.findByLabelText("Search");
  expect(box).toHaveValue("first");
  await act(() => router.navigate({ to: "/r/$slug", params: { slug: "posts" }, search: { search: "second" } }));
  await waitFor(() => expect(screen.getByLabelText("Search")).toHaveValue("second"));
  await act(() => router.history.back());
  await waitFor(() => expect(screen.getByLabelText("Search")).toHaveValue("first"));
  // Typing still drives the URL, and the box keeps what is typed.
  await userEvent.clear(screen.getByLabelText("Search"));
  await userEvent.type(screen.getByLabelText("Search"), "typed");
  await waitFor(() => expect(router.state.location.search).toEqual({ search: "typed" }));
  expect(screen.getByLabelText("Search")).toHaveValue("typed");
});

it("a safe retry fetches a fresh CSRF token when the old one was rotated", async () => {
  let csrfFetches = 0;
  let attempts = 0;
  server.use(
    http.get("*/admin/api/auth/csrf", () => {
      csrfFetches += 1;
      return HttpResponse.json({ csrfToken: `t${csrfFetches}`, userId: "1" });
    }),
    http.post("*/admin/api/resources/posts/actions/:action", ({ request }) => {
      attempts += 1;
      if (attempts === 1) return new HttpResponse(null, { status: 504 });
      if (request.headers.get("X-CSRF-Token") === "t1") return HttpResponse.json({ message: "x", code: "csrf_mismatch" }, { status: 403 });
      return HttpResponse.json({ processed: ["a"], denied: [], missing: [] });
    }),
  );
  try {
    mount();
    await userEvent.click(await screen.findByLabelText("Select Post a"));
    await userEvent.click(screen.getByRole("button", { name: "Publish" }));
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await userEvent.click(await screen.findByRole("button", { name: "Retry safely" }));
    expect(await screen.findByText(/The change was applied/)).toBeInTheDocument();
    expect(attempts).toBe(3);
    expect(csrfFetches).toBe(2);
  } finally {
    server.resetHandlers();
  }
});
