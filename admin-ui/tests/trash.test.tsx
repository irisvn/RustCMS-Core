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
import view from "../../fixtures/contract/kitchen-sink.schema.view.json";
import records from "../../fixtures/contract/kitchen-sink.records.json";
import trash from "../../fixtures/contract/kitchen-sink.trash.json";

const schema = view as unknown as ResourceSchema;
let lists: URLSearchParams[] = [];
let writes: string[] = [];
let abilities = { ...schema.abilities };

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1", name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [schema.resource] })),
  http.get("*/admin/api/resources/gadgets/schema", () => HttpResponse.json({ ...schema, abilities })),
  http.get("*/admin/api/resources/:slug/fields/:field/options", () => HttpResponse.json({ data: [] })),
  http.get("*/admin/api/resources/gadgets/records", ({ request }) => {
    const params = new URL(request.url).searchParams;
    lists.push(params);
    return HttpResponse.json(params.get("trashed") === "only" ? trash : records);
  }),
  http.post("*/admin/api/resources/gadgets/records/:key/restore", ({ params }) => {
    writes.push(`restore ${String(params.key)}`);
    return HttpResponse.json({ data: { ...trash.data[0], trashed: undefined } });
  }),
  http.delete("*/admin/api/resources/gadgets/records/:key/force", ({ params }) => {
    writes.push(`force ${String(params.key)}`);
    return new HttpResponse(null, { status: 204 });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  lists = [];
  writes = [];
  abilities = { ...schema.abilities };
});

function mount(search = "") {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/r/gadgets" + search] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  return router;
}

it("lists the trash and restores or purges records by their abilities", async () => {
  const router = mount();
  await userEvent.selectOptions(await screen.findByLabelText("Trashed records"), "only");
  await waitFor(() => expect(router.state.location.search).toEqual({ trashed: "only" }));
  await waitFor(() => expect(lists.at(-1)?.get("trashed")).toBe("only"));
  const row = (await screen.findByText("Trashed")).closest("tr")!;
  expect(within(row).queryByRole("button", { name: "Delete" })).toBeNull();
  expect(within(row).queryByRole("link", { name: "Edit" })).toBeNull();

  // Trashed records have no detail page.
  expect(within(row).queryByRole("link", { name: "View" })).toBeNull();
  await userEvent.click(within(row).getByRole("button", { name: /Restore/ }));
  await waitFor(() => expect(writes).toEqual([`restore ${trash.data[0]!.key}`]));
  expect(await screen.findByText("Restored.")).toBeInTheDocument();

  await userEvent.click(within(row).getByRole("button", { name: "Delete permanently" }));
  expect(screen.getByText(/will be deleted permanently/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(writes.at(-1)).toBe(`force ${trash.data[0]!.key}`));
});

it("offers no trash without restore or force delete", async () => {
  abilities = { ...abilities, restore: false, forceDelete: false };
  mount();
  await screen.findByRole("heading", { name: schema.resource.pluralLabel });
  expect(screen.queryByLabelText("Trashed records")).toBeNull();
});
