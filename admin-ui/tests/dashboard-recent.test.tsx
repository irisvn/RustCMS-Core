import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";

const AT = "2026-10-24T02:00:45.857073Z";
const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1", name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [] })),
  http.get("*/admin/api/dashboard", () => HttpResponse.json({ resourceLinks: false, widgets: [{ kind: "recent", label: "Latest posts", name: "latest", resource: "posts", span: 1 }] })),
  http.get("*/admin/api/dashboard/widgets/latest", () => HttpResponse.json({
    kind: "recent",
    columns: [
      { name: "status", label: "Status", valueType: "string", options: [{ value: "draft", label: "Draft" }, { value: "live", label: "Published" }] },
      { name: "updated_at", label: "Updated", valueType: "instant", column: { type: "date", name: "updated_at", label: "Updated", valueType: "instant", sortable: true, searchable: false } },
      { name: "published_on", label: "Day", valueType: "date" },
      { name: "category_id", label: "Category", valueType: "i32", column: { type: "text", name: "category_id", label: "Category", valueType: "i32", sortable: false, searchable: false, relationship: { resource: "categories" } } },
    ],
    records: [{
      key: "1", title: "Hello",
      values: { status: "draft", updated_at: AT, published_on: "2026-10-24", category_id: "News" },
      data: { status: "draft", updated_at: AT, published_on: "2026-10-24", category_id: 3 },
    }],
  })),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());

it("shows recent records' values as the resource does: option labels, dates, related titles", async () => {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  const link = await screen.findByRole("link", { name: "Hello" });
  const line = link.nextElementSibling!.textContent!;
  expect(line).toContain("Status: Draft");
  expect(line).not.toContain("draft");
  expect(line).not.toContain(AT);
  expect(line).toContain(`Updated: ${new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short" }).format(new Date(AT))}`);
  expect(line).toContain("Day: Oct 24, 2026");
  expect(line).toContain("Category: News");
});
