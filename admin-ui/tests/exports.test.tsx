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

const schema = { ...(posts as unknown as ResourceSchema), operation: "view", abilities: { ...(posts as unknown as ResourceSchema).abilities, export: true } };
const requests: string[] = [];
let exports: unknown[] = [];

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [schema.resource], settings: [], pages: [] })),
  http.get("*/admin/api/resources/posts/schema", () => HttpResponse.json(schema)),
  http.get("*/admin/api/resources/posts/records", () => HttpResponse.json({ data: [], meta: { page: 1, perPage: 25, total: 0, lastPage: 1 } })),
  http.get("*/admin/api/resources/posts/export", ({ request }) => {
    requests.push(new URL(request.url).search);
    return new HttpResponse("title\r\nFirst\r\n", { headers: { "Content-Type": "text/csv; charset=utf-8" } });
  }),
  http.get("*/admin/api/exports", () => HttpResponse.json(exports)),
);
beforeAll(() => server.listen({ onUnhandledRequest: "bypass" }));
afterAll(() => server.close());
afterEach(() => {
  server.resetHandlers();
  requests.length = 0;
  exports = [];
});

const saved: string[] = [];
beforeEach(() => {
  saved.length = 0;
  URL.createObjectURL = () => "blob:x";
  URL.revokeObjectURL = () => {};
  HTMLAnchorElement.prototype.click = function (this: HTMLAnchorElement) {
    saved.push(this.download);
  };
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

it("exports the list as shown, without its page", async () => {
  mount("/r/posts?search=hello&page=2&sort=title");
  await userEvent.click(await screen.findByRole("button", { name: /Export CSV/ }));
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).toMatch(/^posts-\d{4}-\d{2}-\d{2}\.csv$/);
  const params = new URLSearchParams(requests[0]);
  expect(params.get("search")).toBe("hello");
  expect(params.get("sort")).toBe("title");
  expect(params.has("page")).toBe(false);
});

it("offers a background export when there are too many records, then its file", async () => {
  server.use(
    http.get("*/admin/api/resources/posts/export", () =>
      HttpResponse.json(
        { message: "x", code: "validation_failed", errors: {}, formErrors: ["More than 1000 records"], issues: {}, formIssues: [{ code: "too_many_to_export", params: { max: 1000 } }] },
        { status: 422 },
      ),
    ),
    http.post("*/admin/api/resources/posts/exports", () => {
      exports = [{ id: "e1", resource: "posts", status: "done", rows: 1200, createdAt: "2026-10-22T10:00:00Z", expiresAt: "2026-10-23T10:00:00Z" }];
      return HttpResponse.json({ ...(exports[0] as object), status: "pending" }, { status: 201 });
    }),
    http.get("*/admin/api/exports/e1/file", () => new HttpResponse("title\r\n", { headers: { "Content-Type": "text/csv" } })),
  );
  mount("/r/posts");
  await userEvent.click(await screen.findByRole("button", { name: /Export CSV/ }));
  await userEvent.click(await screen.findByRole("button", { name: "Export in the background" }));
  expect(await screen.findByText(/runs in the background/)).toBeInTheDocument();
  const panel = (await screen.findByText("Exports")).closest("div")!.parentElement!;
  expect(await within(panel).findByText("1200 rows")).toBeInTheDocument();
  await userEvent.click(within(panel).getByRole("button", { name: /Download/ }));
  await waitFor(() => expect(saved).toHaveLength(1));
});
