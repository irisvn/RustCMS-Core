import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { ResourceSchema } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import posts from "../../fixtures/contract/posts.schema.json";

const schema = { ...(posts as unknown as ResourceSchema), operation: "view" };
const view = {
  id: "i1",
  resource: "posts",
  status: "uploaded",
  header: ["Title", "Extra"],
  rows: 2,
  fields: [{ name: "title", label: "Title", required: true, key: false }],
  mapping: { Title: "title" },
  created: 0,
  updated: 0,
  failed: 0,
  report: [],
};
const bodies: { path: string; body: unknown; type: string | null }[] = [];

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [schema.resource], settings: [], pages: [] })),
  http.get("*/admin/api/resources/posts/schema", () => HttpResponse.json(schema)),
  http.post("*/admin/api/resources/posts/imports", async ({ request }) => {
    bodies.push({ path: "upload", body: await request.text(), type: request.headers.get("content-type") });
    return HttpResponse.json(view, { status: 201 });
  }),
  http.post("*/admin/api/imports/i1/preview", async ({ request }) => {
    bodies.push({ path: "preview", body: await request.json(), type: null });
    return HttpResponse.json({ rows: 2, valid: 1, keyed: 0, report: [{ row: 3, field: "title", message: "is required", code: "required" }] });
  }),
  http.post("*/admin/api/imports/i1/run", async ({ request }) => {
    bodies.push({ path: "run", body: await request.json(), type: null });
    return HttpResponse.json({ ...view, status: "running", mapping: { Title: "title" } });
  }),
  http.get("*/admin/api/imports/i1", () =>
    HttpResponse.json({ ...view, status: "done", created: 1, failed: 1, report: [{ row: 3, field: "title", message: "is required", code: "required" }] }),
  ),
);
beforeAll(() => server.listen({ onUnhandledRequest: "bypass" }));
afterAll(() => server.close());

it("uploads, checks and imports a file, then shows the outcome", async () => {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/r/posts/import"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  const file = new File(["Title,Extra\r\nFirst,x\r\n,y\r\n"], "posts.csv", { type: "text/csv" });
  await userEvent.upload(await screen.findByLabelText("CSV file"), file);
  expect(await screen.findByText("2 rows")).toBeInTheDocument();
  expect(bodies[0]).toMatchObject({ path: "upload", type: "text/csv", body: "Title,Extra\r\nFirst,x\r\n,y\r\n" });
  // Columns: the suggested field, or none.
  expect(screen.getByRole("combobox", { name: "Title" })).toHaveValue("title");
  expect(screen.getByRole("combobox", { name: "Extra" })).toHaveValue("");
  expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
  await userEvent.click(screen.getByRole("button", { name: "Check" }));
  expect(await screen.findByText("1 of 2 rows can be imported.")).toBeInTheDocument();
  expect(screen.getAllByText("is required").length).toBeGreaterThan(0);
  await userEvent.click(screen.getByRole("button", { name: "Import" }));
  await waitFor(() => expect(bodies.map((b) => b.path)).toContain("run"));
  expect(bodies.find((b) => b.path === "run")!.body).toEqual({ mapping: { Title: "title" } });
  expect(await screen.findByText("Done: 1 created, 0 updated, 1 failed.")).toBeInTheDocument();
});

it("follows an import that started although its answer was lost", async () => {
  server.use(
    http.post("*/admin/api/imports/i1/run", () =>
      HttpResponse.json({ code: "import_started", reason: "started", message: "This import has already started." }, { status: 409 }),
    ),
    http.post("*/admin/api/imports/i1/preview", () => HttpResponse.json({ rows: 2, valid: 2, keyed: 0, report: [] })),
    http.get("*/admin/api/imports/i1", () => HttpResponse.json({ ...view, status: "running", created: 1 })),
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/r/posts/import"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  await userEvent.upload(await screen.findByLabelText("CSV file"), new File(["Title\r\nA\r\nB\r\n"], "p.csv", { type: "text/csv" }));
  await userEvent.click(await screen.findByRole("button", { name: "Check" }));
  await userEvent.click(await screen.findByRole("button", { name: "Import" }));
  expect(await screen.findByText(/Importing… 1 of 2 rows/)).toBeInTheDocument();
  expect(screen.queryByText("This import has already started.")).not.toBeInTheDocument();
});
