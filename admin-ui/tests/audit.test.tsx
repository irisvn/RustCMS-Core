import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { AuditEntryView } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import posts from "../../fixtures/contract/posts.schema.json";

const entry = (id: string, over: Partial<AuditEntryView> = {}): AuditEntryView => ({
  id,
  at: "2026-10-18T08:00:00.000000Z",
  event: "updated",
  subjectType: "posts",
  subjectKey: "p1",
  actor: { kind: "user", id: "7", name: "Editor", credential: "session" },
  requestId: "req-1",
  changes: { title: ["Old", "New"] },
  ...over,
});

let queries: URLSearchParams[] = [];
const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({
      brand: "T",
      user: { id: "1", name: "A", email: "a@x.y" },
      navigation: [],
      system: { users: false, roles: false, createUsers: false, createRoles: false, audit: true },
      settings: [],
    }),
  ),
  http.get("*/admin/api/audit", ({ request }) => {
    const query = new URL(request.url).searchParams;
    queries.push(query);
    if (query.get("cursor") === "c1") {
      return HttpResponse.json({
        data: [entry("3", { event: "orders.refunded", changes: undefined, actor: { kind: "job", id: "7", name: "Editor", credential: "queue" } })],
        meta: {},
      });
    }
    return HttpResponse.json({
      data: [entry("1"), entry("2", { event: "action:publish", actor: { kind: "system", credential: "console" }, changes: { overflow: true, fields: ["body"] } })],
      meta: { nextCursor: "c1" },
    });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => {
  server.resetHandlers();
  queries = [];
});

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
}

it("lists entries, filters them and loads older ones", async () => {
  mount("/system/audit");
  const list = await screen.findByRole("list", { name: "Audit entries" });
  expect(within(list).getByText("Updated")).toBeInTheDocument();
  expect(within(list).getByText("Action publish")).toBeInTheDocument();
  expect(within(list).getByText("System")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Audit log" })).toHaveAttribute("href", "/system/audit");

  // Details: the changes, as stored.
  await userEvent.click(within(list).getAllByRole("button", { name: "Show details" })[0]!);
  expect(within(list).getByRole("rowheader", { name: "title" })).toBeInTheDocument();
  expect(within(list).getByText("New")).toBeInTheDocument();
  expect(within(list).getByText("req-1")).toBeInTheDocument();

  await userEvent.click(screen.getByRole("button", { name: "Load more" }));
  expect(await within(list).findByText("orders.refunded")).toBeInTheDocument();
  expect(within(list).getByText("Background job for Editor")).toBeInTheDocument();
  expect(queries.at(-1)?.get("cursor")).toBe("c1");
  expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();

  await userEvent.type(screen.getByLabelText("Event"), "created");
  await userEvent.type(screen.getByLabelText("Subject key"), "ignored without a type");
  await userEvent.click(screen.getByRole("button", { name: "Filter" }));
  await screen.findByRole("list", { name: "Audit entries" });
  const last = queries.at(-1)!;
  expect(last.get("event")).toBe("created");
  expect(last.has("subject_key")).toBe(false);
});

it("shows a record's history only with the ability", async () => {
  const record = { key: "p1", title: "Post", abilities: { view: true, update: false, delete: false }, data: { title: "Post" } };
  server.use(
    http.get("*/admin/api/resources/posts/schema", () => HttpResponse.json({ ...posts, operation: "view" })),
    http.get("*/admin/api/resources/posts/records/p1", () => HttpResponse.json({ data: record })),
    http.get("*/admin/api/resources/posts/records/p1/history", () => HttpResponse.json({ data: [entry("9")], meta: {} })),
  );
  mount("/r/posts/records/p1");
  await userEvent.click(await screen.findByRole("button", { name: "Show history" }));
  expect(await screen.findByText("Updated")).toBeInTheDocument();
});

it("has no history section without the ability", async () => {
  const schema = structuredClone(posts) as typeof posts & { abilities: Record<string, boolean> };
  schema.abilities.history = false;
  const record = { key: "p1", title: "Post", abilities: { view: true, update: false, delete: false }, data: { title: "Post" } };
  server.use(
    http.get("*/admin/api/resources/posts/schema", () => HttpResponse.json({ ...schema, operation: "view" })),
    http.get("*/admin/api/resources/posts/records/p1", () => HttpResponse.json({ data: record })),
  );
  mount("/r/posts/records/p1");
  await screen.findByRole("heading", { name: "Post" });
  expect(screen.queryByRole("button", { name: "Show history" })).toBeNull();
});
