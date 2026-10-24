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

const schema = posts as unknown as ResourceSchema;
const runs: { action: string; body: unknown }[] = [];
const row = (key: string, abilities: Record<string, boolean>) => ({ key, title: `Post ${key}`, abilities: { view: true, ...abilities }, data: { title: `Post ${key}` } });

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1", name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [schema.resource] })),
  http.get("*/admin/api/resources/posts/schema", () => HttpResponse.json({ ...schema, operation: "view" })),
  http.get("*/admin/api/resources/posts/records", () => HttpResponse.json({
    data: [
      // Published: unpublish is offered (the server decides per row).
      row("a", { touch: true, unpublish: true, archive: true }),
      // A draft: only the primary action.
      row("b", { touch: true, unpublish: false, archive: false }),
      // Nothing primary offered: no labelled action, the rest in the menu.
      row("c", { touch: false, archive: true }),
    ],
    meta: { page: 1, perPage: 25, total: 3, lastPage: 1 },
  })),
  http.post("*/admin/api/resources/posts/actions/:action", async ({ request, params }) => {
    runs.push({ action: String(params.action), body: await request.json() });
    return HttpResponse.json({ processed: ["a"], denied: [], missing: [] });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());

function mount() {
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
}

const rowOf = (title: string) => screen.getByText(title).closest("tr")!;

it("shows one labelled action per row and the others in a menu, destructive ones last", async () => {
  mount();
  await screen.findByText("Post a");
  const a = rowOf("Post a");
  expect(within(a).getByRole("button", { name: "Touch" })).toBeInTheDocument();
  expect(within(a).queryByRole("button", { name: "Archive" })).toBeNull();
  await userEvent.click(within(a).getByRole("button", { name: "More actions for Post a" }));
  const menu = await screen.findByRole("menu");
  expect([...menu.children].map((c) => c.getAttribute("role") === "separator" ? "---" : c.textContent)).toEqual(["Archive", "---", "Unpublish"]);
  expect(within(menu).getByRole("menuitem", { name: "Unpublish" })).toHaveClass("text-destructive");
  await userEvent.click(within(menu).getByRole("menuitem", { name: "Unpublish" }));
  await waitFor(() => expect(runs).toEqual([{ action: "unpublish", body: { keys: ["a"], data: {} } }]));

  // Hidden on a row by the server: neither labelled nor in a menu.
  const b = rowOf("Post b");
  expect(within(b).getByRole("button", { name: "Touch" })).toBeInTheDocument();
  expect(within(b).queryByRole("button", { name: /More actions/ })).toBeNull();
  const c = rowOf("Post c");
  expect(within(c).queryByRole("button", { name: "Touch" })).toBeNull();
  await userEvent.click(within(c).getByRole("button", { name: "More actions for Post c" }));
  expect(within(await screen.findByRole("menu")).getAllByRole("menuitem").map((i) => i.textContent)).toEqual(["Archive"]);
});
