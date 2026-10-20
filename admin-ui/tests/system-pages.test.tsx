import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { groupPermissions } from "@/api/system";

const server = setupServer(
  http.get("*/admin/api/system/permission-presets", () => HttpResponse.json([])),
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "5" }, guard: "admin", permissions: ["users.create"], superAdmin: false })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "5" })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({ brand: "T", user: { id: "5", name: "C", email: "c@x.y" }, navigation: [], system: { users: false, roles: false } }),
  ),
  http.get("*/admin/api/system/roles", () => HttpResponse.json({ message: "no", code: "forbidden" }, { status: 403 })),
  http.post("*/admin/api/system/users", () => new HttpResponse(null, { status: 204 })),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => server.resetHandlers());

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

it("a creator who may not view users stays on the form after creating one", async () => {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/system/users/create"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  await userEvent.type(await screen.findByLabelText("Name"), "New");
  await userEvent.type(screen.getByLabelText("Email"), "new@x.y");
  await userEvent.type(screen.getByLabelText("Password"), "long enough password");
  await userEvent.click(screen.getByRole("button", { name: "Create user" }));
  expect(await screen.findByText("new@x.y was created.")).toBeInTheDocument();
  await waitFor(() => expect(router.state.location.pathname).toBe("/system/users/create"));
  expect(screen.getByLabelText("Email")).toHaveValue("");
});

it("connects user validation errors to their matching labels", async () => {
  server.use(http.post("*/admin/api/system/users", () =>
    HttpResponse.json({ message: "x", code: "validation_failed", errors: { email: ["This email is already in use."] } }, { status: 422 }),
  ));
  mount("/system/users/create");
  await userEvent.type(await screen.findByLabelText("Email"), "used@x.y");
  await userEvent.click(screen.getByRole("button", { name: "Create user" }));
  await screen.findByText("This email is already in use.");
  expect(screen.getByLabelText("Email")).toHaveAccessibleDescription("This email is already in use.");
  expect(screen.getByLabelText("Email")).toHaveAttribute("aria-invalid", "true");
  expect(screen.getByLabelText("Name")).not.toHaveAttribute("aria-invalid", "true");
});

it("keeps named system tables inside their local overflow container", async () => {
  server.use(http.get("*/admin/api/system/users", () => HttpResponse.json({
    data: [], meta: { page: 1, perPage: 25, total: 0, lastPage: 1 },
  })));
  mount("/system/users");
  const table = await screen.findByRole("table", { name: "Users" });
  expect(table.parentElement).toHaveClass("overflow-x-auto");
  expect(screen.getByLabelText("Search users")).toBeInTheDocument();
  expect(screen.queryByRole("link", { name: "New user" })).not.toBeInTheDocument();
});

it("keeps grouped permission labels and grantability guards", async () => {
  server.use(http.get("*/admin/api/system/permissions", () => HttpResponse.json([
    { name: "posts.view", group: "posts", grantable: true, area: "resources", areaLabel: "Resources", areaSort: 0 },
    { name: "posts.delete", group: "posts", grantable: false, area: "resources", areaLabel: "Resources", areaSort: 0 },
  ])));
  mount("/system/roles/create");
  const view = await screen.findByRole("checkbox", { name: "posts.view" });
  expect(screen.getByRole("group", { name: "posts" })).toContainElement(view);
  expect(view).toHaveAccessibleDescription(/You can only change permissions you hold/);
  expect(view).not.toBeDisabled();
  expect(screen.getByRole("checkbox", { name: "posts.delete" })).toBeDisabled();
});

it("groups permissions by resource", () => {
  const groups = groupPermissions([
    { name: "posts.view", group: "posts", grantable: true, area: "resources", areaLabel: "Resources", areaSort: 0 },
    { name: "categories.view", grantable: false, area: "resources", areaLabel: "Resources", areaSort: 0 },
    { name: "posts.create", group: "posts", grantable: true, area: "resources", areaLabel: "Resources", areaSort: 0 },
  ]);
  expect(groups.map(([g, l]) => [g, l.map((p) => p.name)])).toEqual([
    ["categories", ["categories.view"]],
    ["posts", ["posts.create", "posts.view"]],
  ]);
});
