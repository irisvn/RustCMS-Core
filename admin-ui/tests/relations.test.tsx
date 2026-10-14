import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { ResourceSchema } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import posts from "../../fixtures/contract/posts.schema.json";

// Categories (parent) have posts (children) through `category_id`.
const child = posts as unknown as ResourceSchema;
const parent: ResourceSchema = {
  ...child,
  resource: { ...child.resource, slug: "categories", label: "Category", pluralLabel: "Categories" },
  operation: "view",
  abilities: { view: true, viewAny: true },
  relations: [{ name: "posts", label: "Posts", resource: "posts", field: "category_id", create: true }],
};
const row = (key: string, title: string) => ({ key, title, abilities: { view: true }, data: { title, category_id: 1 } });
const lists: URLSearchParams[] = [];
const creates: { path: string; body: unknown; key: string | null }[] = [];

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1", name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [parent.resource, child.resource] })),
  http.get("*/admin/api/resources/categories/schema", () => HttpResponse.json(parent)),
  http.get("*/admin/api/resources/posts/schema", ({ request }) => {
    const operation = new URL(request.url).searchParams.get("operation");
    return HttpResponse.json({ ...child, operation, abilities: { ...child.abilities, create: true } });
  }),
  http.get("*/admin/api/resources/categories/records/1", () => HttpResponse.json({ data: { key: "1", title: "News", abilities: { view: true }, data: { title: "News" } } })),
  http.get("*/admin/api/resources/categories/records/1/relations/posts", ({ request }) => {
    lists.push(new URL(request.url).searchParams);
    return HttpResponse.json({ data: [row("a", "First post")], meta: { page: 1, perPage: 10, total: 1, lastPage: 1 } });
  }),
  http.post("*/admin/api/resources/categories/records/1/relations/posts", async ({ request }) => {
    creates.push({ path: new URL(request.url).pathname, body: await request.json(), key: request.headers.get("Idempotency-Key") });
    return HttpResponse.json({ data: row("b", "Second") }, { status: 201 });
  }),
  http.get("*/admin/api/resources/:slug/fields/:field/options", () => HttpResponse.json({ data: [] })),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());

function mount(path: string) {
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

it("lists related records on the parent's page and links to them", async () => {
  mount("/r/categories/records/1");
  const table = await screen.findByRole("table", { name: "Posts" });
  expect(within(table).getByText("First post")).toBeInTheDocument();
  // The parent column is not repeated in every row.
  expect(within(table).queryByRole("columnheader", { name: "Category" })).toBeNull();
  expect(within(table).getByRole("link", { name: "View" })).toHaveAttribute("href", "/r/posts/records/a");
  expect(screen.getByRole("link", { name: /New/ })).toHaveAttribute("href", "/r/posts/create?via=categories&record=1&relation=posts");
});

it("creates through the relation without the parent's field, then returns to the parent", async () => {
  const router = mount("/r/posts/create?via=categories&record=1&relation=posts");
  await userEvent.type(await screen.findByLabelText(/^Title/), "Second");
  // The server sets `category_id`: no input for it.
  expect(screen.queryByText("Category")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Create" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/r/categories/records/1"));
  expect(creates).toHaveLength(1);
  expect(creates[0]!.path).toBe("/admin/api/resources/categories/records/1/relations/posts");
  expect(creates[0]!.body).not.toHaveProperty("category_id");
  expect(creates[0]!.key).toBeTruthy();
});

it("a retried create through the relation also returns to the parent", async () => {
  let status = 504;
  server.use(
    http.post("*/admin/api/resources/categories/records/1/relations/posts", () =>
      status === 504 ? new HttpResponse(null, { status: 504 }) : HttpResponse.json({ data: row("c", "Third") }, { status: 201 }),
    ),
  );
  try {
    const router = mount("/r/posts/create?via=categories&record=1&relation=posts");
    await userEvent.type(await screen.findByLabelText(/^Title/), "Third");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    status = 201;
    await userEvent.click(await screen.findByRole("button", { name: "Retry safely" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/r/categories/records/1"));
  } finally {
    server.resetHandlers();
  }
});

it("refuses a relation the parent does not announce", async () => {
  mount("/r/posts/create?via=categories&record=1&relation=comments");
  expect(await screen.findByText(/not allowed|forbidden|permission/i)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Create" })).toBeNull();
});
