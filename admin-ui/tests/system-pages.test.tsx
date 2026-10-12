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

it("groups permissions by resource", () => {
  const groups = groupPermissions([
    { name: "posts.view", group: "posts", grantable: true },
    { name: "categories.view", grantable: false },
    { name: "posts.create", group: "posts", grantable: true },
  ]);
  expect(groups.map(([g, l]) => [g, l.map((p) => p.name)])).toEqual([
    ["categories", ["categories.view"]],
    ["posts", ["posts.create", "posts.view"]],
  ]);
});
