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

function mount() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/r/posts"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
}

it("pauses actions after one with an unknown outcome", async () => {
  server.use(http.post("*/admin/api/resources/posts/actions/:action", () => new HttpResponse(null, { status: 504 })));
  mount();
  await userEvent.click(await screen.findByLabelText("Select Post a"));
  await userEvent.click(screen.getByRole("button", { name: "Publish" }));
  await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
  expect(await screen.findByText(/Actions are paused/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Publish" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
  // Leaving and coming back does not lift the pause; only a reload does.
  await act(() => router.navigate({ to: "/" }));
  await act(() => router.navigate({ to: "/r/$slug", params: { slug: "posts" } }));
  expect(await screen.findByText(/Actions are paused/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /New post/ })).toBeNull();
  server.resetHandlers();
});

it("never acts on rows of the previous page while the next one loads", async () => {
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
