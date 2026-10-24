import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { ResourceSchema } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { LinksWidget, addLink, linksValue, removeLink } from "@/renderers/links";
import postsList from "../../fixtures/contract/posts.schema.json";
import postsView from "../../fixtures/contract/posts.schema.view.json";

// Posts link tags (pivot `position`, write-only `note`); posts have a `move` action with a form.
const view = postsView as unknown as ResourceSchema;
const list = postsList as unknown as ResourceSchema;
const tags: ResourceSchema = {
  ...list,
  resource: { slug: "tags", label: "Tag", pluralLabel: "Tags", titleField: "label" },
  table: {
    ...list.table,
    actions: [],
    filters: [],
    defaultSort: undefined,
    columns: [{ name: "label", label: "Label", type: "text", valueType: "string", sortable: false, searchable: true }],
  },
};
const POST = "0192b7c8-0000-7000-8000-000000000001";
const writes: { method: string; path: string; body: unknown }[] = [];
const record = (key: string, title: string, extra = {}) => ({ key, title, abilities: { view: true, update: true, delete: true, move: true }, data: { title, ...extra } });

const server = setupServer(
  http.get("*/admin/api/system/permission-presets", () => HttpResponse.json([])),
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [list.resource], system: { users: false, roles: true, createUsers: false, createRoles: true, audit: false, queue: false }, settings: [], pages: [] })),
  http.get("*/admin/api/resources/posts/schema", ({ request }) =>
    HttpResponse.json(new URL(request.url).searchParams.has("key") ? view : { ...list, operation: "view" }),
  ),
  http.get("*/admin/api/resources/tags/schema", () => HttpResponse.json({ ...tags, operation: "view" })),
  http.get(`*/admin/api/resources/posts/records/${POST}`, () => HttpResponse.json({ data: record(POST, "Mine") })),
  http.get("*/admin/api/resources/posts/records", () =>
    HttpResponse.json({ data: [record(POST, "Mine"), record("p2", "Other")], meta: { page: 1, perPage: 25, total: 2, lastPage: 1 } }),
  ),
  http.get(`*/admin/api/resources/posts/records/${POST}/relations/tags`, () =>
    HttpResponse.json({
      data: [{ key: "rust", title: "RUST", abilities: { view: true }, data: { label: "RUST" } }],
      meta: { page: 1, perPage: 10, total: 1, lastPage: 1 },
      pivot: { rust: { position: 3 } },
    }),
  ),
  http.get(`*/admin/api/resources/posts/records/${POST}/relations/tags/options`, () =>
    HttpResponse.json({ data: [{ value: "rust", label: "RUST", attached: true }, { value: "sql", label: "SQL", attached: false }] }),
  ),
  http.post(`*/admin/api/resources/posts/records/${POST}/relations/tags/:op`, async ({ request, params }) => {
    writes.push({ method: "POST", path: String(params.op), body: await request.json() });
    return HttpResponse.json({ processed: ["sql"], missing: [] });
  }),
  http.patch(`*/admin/api/resources/posts/records/${POST}/relations/tags/links/:target`, async ({ request, params }) => {
    writes.push({ method: "PATCH", path: String(params.target), body: await request.json() });
    return HttpResponse.json({ processed: ["rust"], missing: [] });
  }),
  http.get("*/admin/api/resources/posts/actions/move/fields/category_id/options", () =>
    HttpResponse.json({ data: [{ value: "7", label: "News" }] }),
  ),
  http.get("*/admin/api/resources/:slug/records/:key/history", () => HttpResponse.json({ data: [], meta: {} })),
);
beforeAll(() => server.listen({ onUnhandledRequest: "bypass" }));
afterAll(() => server.close());
afterEach(() => {
  server.resetHandlers();
  writes.length = 0;
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
  return router;
}

it("shows linked records with their pivot columns, attaches and detaches", async () => {
  mount(`/r/posts/records/${POST}`);
  const table = await screen.findByRole("table", { name: "Tags" });
  expect(within(table).getByText("RUST")).toBeInTheDocument();
  expect(within(table).getByRole("columnheader", { name: "Position" })).toBeInTheDocument();
  expect(within(table).getByText("3")).toBeInTheDocument();

  // Attach: already linked records cannot be chosen again; the pivot form is sent along.
  await userEvent.click(screen.getByRole("button", { name: /Attach/ }));
  const dialog = await screen.findByRole("dialog", { name: "Attach to Tags" });
  const sql = await within(dialog).findByRole("checkbox", { name: /SQL/ });
  expect(within(dialog).getByRole("checkbox", { name: /RUST/ })).toBeDisabled();
  expect(within(dialog).getByRole("button", { name: "Attach" })).toBeDisabled();
  await userEvent.click(sql);
  await userEvent.type(within(dialog).getByLabelText(/^Position/), "2");
  await userEvent.click(within(dialog).getByRole("button", { name: "Attach" }));
  await waitFor(() => expect(writes).toHaveLength(1));
  expect(writes[0]).toEqual({ method: "POST", path: "attach", body: { keys: ["sql"], pivot: { position: 2 } } });
  expect(await screen.findByText("1 attached.")).toBeInTheDocument();

  // Detach the selected rows, after confirming.
  await userEvent.click(within(table).getByRole("checkbox", { name: "Select RUST" }));
  await userEvent.click(screen.getByRole("button", { name: /Detach selected/ }));
  const confirm = await screen.findByRole("dialog", { name: "Detach" });
  await userEvent.click(within(confirm).getByRole("button", { name: "Detach" }));
  await waitFor(() => expect(writes).toHaveLength(2));
  expect(writes[1]).toEqual({ method: "POST", path: "detach", body: { keys: ["rust"] } });
});

it("leaves untouched pivot columns to the database when attaching", async () => {
  mount(`/r/posts/records/${POST}`);
  await userEvent.click(await screen.findByRole("button", { name: /Attach/ }));
  const dialog = await screen.findByRole("dialog", { name: "Attach to Tags" });
  await userEvent.click(await within(dialog).findByRole("checkbox", { name: /SQL/ }));
  await userEvent.click(within(dialog).getByRole("button", { name: "Attach" }));
  await waitFor(() => expect(writes).toHaveLength(1));
  expect(writes[0]).toEqual({ method: "POST", path: "attach", body: { keys: ["sql"], pivot: {} } });
});

it("edits a link's pivot columns", async () => {
  mount(`/r/posts/records/${POST}`);
  await userEvent.click(await screen.findByRole("button", { name: "Edit link: RUST" }));
  const dialog = await screen.findByRole("dialog", { name: "Edit link" });
  const position = within(dialog).getByLabelText(/^Position/);
  expect(position).toHaveValue("3");
  await userEvent.clear(position);
  await userEvent.type(position, "5");
  await userEvent.click(within(dialog).getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(writes).toHaveLength(1));
  expect(writes[0]).toEqual({ method: "PATCH", path: "rust", body: { position: 5 } });
});

it("runs an action with a form and shows the server's field errors", async () => {
  const runs: unknown[] = [];
  let first = true;
  server.use(
    http.post("*/admin/api/resources/posts/actions/move", async ({ request }) => {
      runs.push(await request.json());
      if (first) {
        first = false;
        return HttpResponse.json({ message: "x", code: "validation_failed", errors: { note: ["may not be longer than 10 characters"] } }, { status: 422 });
      }
      return HttpResponse.json({ processed: [POST], denied: [], missing: [] });
    }),
  );
  mount("/r/posts");
  const rowOf = async (title: string) => (await screen.findByText(title)).closest("tr")!;
  await userEvent.click(within(await rowOf("Mine")).getByRole("checkbox"));
  await userEvent.click(screen.getByRole("button", { name: "Move to category…" }));
  const dialog = await screen.findByRole("dialog", { name: "Move to category…" });
  // The relationship select reads the action's own options.
  within(dialog).getByRole("combobox", { name: /Category/ }).focus();
  await userEvent.keyboard("{ArrowDown}");
  await screen.findByRole("option", { name: "News" });
  await userEvent.keyboard("{Enter}");
  await userEvent.type(within(dialog).getByLabelText(/^Note/), "long note!");
  await userEvent.click(within(dialog).getByRole("button", { name: "Run" }));
  expect(await within(dialog).findByText("may not be longer than 10 characters")).toBeInTheDocument();
  await userEvent.click(within(dialog).getByRole("button", { name: "Run" }));
  await waitFor(() => expect(runs).toHaveLength(2));
  expect(runs[1]).toMatchObject({ keys: [POST], data: { category_id: "7", mode: "move", note: "long note!" } });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Move to category…" })).toBeNull());
});

it("edits an instant action field in UTC and sends it as RFC 3339", async () => {
  const runs: unknown[] = [];
  server.use(
    http.get("*/admin/api/resources/posts/records", () =>
      HttpResponse.json({ data: [{ ...record(POST, "Mine"), abilities: { view: true, schedule: true } }], meta: { page: 1, perPage: 25, total: 1, lastPage: 1 } }),
    ),
    http.post("*/admin/api/resources/posts/actions/schedule", async ({ request }) => {
      runs.push(await request.json());
      return HttpResponse.json({ processed: [POST], denied: [], missing: [] });
    }),
  );
  mount("/r/posts");
  await userEvent.click(await screen.findByRole("button", { name: "Schedule…" }));
  const dialog = await screen.findByRole("dialog", { name: "Schedule…" });
  const at = within(dialog).getByLabelText(/^Publish at/);
  expect(at).toHaveAttribute("type", "datetime-local");
  // Labelled UTC, like the form's picker of an instant column.
  expect(within(dialog).getByText("UTC")).toBeInTheDocument();
  fireEvent.change(at, { target: { value: "2026-10-12T09:30" } });
  await userEvent.click(within(dialog).getByRole("button", { name: "Run" }));
  await waitFor(() => expect(runs).toHaveLength(1));
  expect(runs[0]).toEqual({ keys: [POST], data: { at: "2026-10-12T09:30:00Z" } });
});

it("links records from the create form and sends only the change", async () => {
  const created: unknown[] = [];
  server.use(
    http.get("*/admin/api/resources/posts/schema", () => HttpResponse.json({ ...list, operation: "create" })),
    http.get("*/admin/api/resources/posts/relations/tags/options", () =>
      HttpResponse.json({ data: [{ value: "rust", label: "RUST", attached: false }, { value: "sql", label: "SQL", attached: false }] }),
    ),
    http.post("*/admin/api/resources/posts/records", async ({ request }) => {
      created.push(await request.json());
      return HttpResponse.json({ data: record(POST, "New") }, { status: 201 });
    }),
  );
  mount("/r/posts/create");
  await userEvent.type(await screen.findByLabelText(/^Title/), "New");
  expect(screen.getByText("None linked.")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("combobox", { name: "Tags" }));
  await userEvent.click(await screen.findByRole("button", { name: "SQL" }));
  const chosen = screen.getByRole("list", { name: "Tags" });
  expect(within(chosen).getByText("SQL")).toBeInTheDocument();
  // A record chosen here can be put back before saving.
  await userEvent.click(screen.getByRole("combobox", { name: "Tags" }));
  await userEvent.click(await screen.findByRole("button", { name: "RUST" }));
  await userEvent.click(within(screen.getByRole("list", { name: "Tags" })).getByRole("button", { name: "Remove RUST" }));
  await userEvent.click(screen.getByRole("button", { name: "Create" }));
  await waitFor(() => expect(created).toHaveLength(1));
  expect(created[0]).toMatchObject({ title: "New", tags: { attach: ["sql"] } });
});

it("changes the links of a record from its edit form", async () => {
  const patches: unknown[] = [];
  const edit = structuredClone(list) as ResourceSchema;
  const tagsInput = edit.form.components.find((c) => "name" in c && c.name === "tags") as { links?: { attach: boolean; detach: boolean } };
  tagsInput.links = { attach: true, detach: true };
  server.use(
    http.get("*/admin/api/resources/posts/schema", ({ request }) =>
      HttpResponse.json(new URL(request.url).searchParams.get("operation") === "edit" ? { ...edit, operation: "edit", abilities: { ...edit.abilities, view: true, update: true } } : { ...list, operation: "view" }),
    ),
    http.patch(`*/admin/api/resources/posts/records/${POST}`, async ({ request }) => {
      patches.push(await request.json());
      return HttpResponse.json({ data: record(POST, "Mine") });
    }),
  );
  mount(`/r/posts/records/${POST}/edit`);
  // The current links come from the relation's list.
  const chosen = await screen.findByRole("list", { name: "Tags" });
  await userEvent.click(within(chosen).getByRole("button", { name: "Remove RUST" }));
  expect(screen.queryByRole("list", { name: "Tags" })).toBeNull();
  // The removed chip's button is gone: focus moves to the search.
  await waitFor(() => expect(screen.getByRole("combobox", { name: "Tags" })).toHaveFocus());
  await userEvent.click(screen.getByRole("combobox", { name: "Tags" }));
  await userEvent.click(await screen.findByRole("button", { name: "SQL" }));
  await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(patches).toHaveLength(1));
  expect(patches[0]).toEqual({ tags: { attach: ["sql"], detach: ["rust"] } });
});

it("undoes a pending removal without the right to attach", async () => {
  const patches: unknown[] = [];
  const edit = structuredClone(list) as ResourceSchema;
  const tagsInput = edit.form.components.find((c) => "name" in c && c.name === "tags") as { links?: { attach: boolean; detach: boolean } };
  tagsInput.links = { attach: false, detach: true };
  server.use(
    http.get("*/admin/api/resources/posts/schema", () =>
      HttpResponse.json({ ...edit, operation: "edit", abilities: { ...edit.abilities, view: true, update: true } }),
    ),
    http.patch(`*/admin/api/resources/posts/records/${POST}`, async ({ request }) => {
      patches.push(await request.json());
      return HttpResponse.json({ data: record(POST, "Mine") });
    }),
  );
  mount(`/r/posts/records/${POST}/edit`);
  const chosen = await screen.findByRole("list", { name: "Tags" });
  expect(screen.queryByRole("combobox", { name: "Tags" })).toBeNull();
  // Without the search, the input itself is the field (labelled, focusable).
  const field = screen.getByRole("group", { name: "Tags" });
  expect(field).toHaveAttribute("id", "field-tags");
  await userEvent.click(within(chosen).getByRole("button", { name: "Remove RUST" }));
  await waitFor(() => expect(field).toHaveFocus());
  const removed = screen.getByRole("list", { name: "Removed, not saved yet" });
  await userEvent.click(within(removed).getByRole("button", { name: "Keep RUST" }));
  expect(within(screen.getByRole("list", { name: "Tags" })).getByText("RUST")).toBeInTheDocument();
  // Nothing changed: only the title is sent.
  await userEvent.type(screen.getByLabelText(/^Title/), "!");
  await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(patches).toHaveLength(1));
  expect(patches[0]).toEqual({ title: "Mine!" });
});

it("lets a blind editor attach without showing the current links", async () => {
  const patches: unknown[] = [];
  const edit = structuredClone(list) as ResourceSchema;
  const tagsInput = edit.form.components.find((c) => "name" in c && c.name === "tags") as { links?: { attach: boolean; detach: boolean } };
  tagsInput.links = { attach: true, detach: false };
  server.use(
    http.get("*/admin/api/resources/posts/schema", () =>
      HttpResponse.json({ ...edit, operation: "edit", abilities: { ...edit.abilities, view: false, update: true } }),
    ),
    http.get(`*/admin/api/resources/posts/records/${POST}/relations/tags`, () =>
      HttpResponse.json({ message: "Forbidden.", code: "forbidden" }, { status: 403 }),
    ),
    http.patch(`*/admin/api/resources/posts/records/${POST}`, async ({ request }) => {
      patches.push(await request.json());
      return HttpResponse.json({ data: { ...record(POST, "Mine"), abilities: { view: false, update: true } } });
    }),
  );
  mount(`/r/posts/records/${POST}/edit`);
  expect(await screen.findByText("You may link more records here, but not see the current links.")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("combobox", { name: "Tags" }));
  await userEvent.click(await screen.findByRole("button", { name: "SQL" }));
  await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(patches).toHaveLength(1));
  expect(patches[0]).toEqual({ tags: { attach: ["sql"] } });
});

it("stops offering records once a save would attach the server's maximum", () => {
  const many = JSON.stringify({ attach: Array.from({ length: 100 }, (_, i) => `t${i}`) });
  const input = (list.form.components.find((c) => "name" in c && c.name === "tags") ?? null) as unknown as Parameters<typeof LinksWidget>[0]["input"];
  const queryClient = new QueryClient();
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <LinksWidget id="field-tags" input={input} slug="posts" value={many} onChange={() => {}} invalid={false} onBlur={() => {}} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  expect(screen.getByText("At most 100 records are linked per save: save, then add more.")).toBeInTheDocument();
  expect(screen.queryByRole("combobox")).toBeNull();
});

it("shows linked records whatever their key, `__proto__` included", async () => {
  const edit = structuredClone(list) as ResourceSchema;
  const tagsInput = edit.form.components.find((c) => "name" in c && c.name === "tags") as { links?: { attach: boolean; detach: boolean } };
  tagsInput.links = { attach: true, detach: true };
  server.use(
    http.get("*/admin/api/resources/posts/schema", () =>
      HttpResponse.json({ ...edit, operation: "edit", abilities: { ...edit.abilities, view: true, update: true } }),
    ),
    http.get(`*/admin/api/resources/posts/records/${POST}/relations/tags`, () =>
      HttpResponse.json({ data: [{ key: "__proto__", title: "Proto", abilities: { view: true }, data: {} }], meta: { page: 1, perPage: 100, total: 1, lastPage: 1 } }),
    ),
  );
  mount(`/r/posts/records/${POST}/edit`);
  const chosen = await screen.findByRole("list", { name: "Tags" });
  expect(within(chosen).getByText("Proto")).toBeInTheDocument();
});

it("keeps a link change disjoint when the current links change meanwhile", () => {
  // SQL was chosen here, then someone else linked it: removing its chip only drops the attach…
  expect(removeLink({ attach: ["sql"], detach: [] }, "sql", new Set(["sql"]), false)).toEqual({ attach: [], detach: [] });
  // …or, with `detach`, unlinks it, never both.
  expect(removeLink({ attach: ["sql"], detach: [] }, "sql", new Set(["sql"]), true)).toEqual({ attach: [], detach: ["sql"] });
  expect(addLink({ attach: [], detach: ["rust"] }, "rust", new Set(["rust"]))).toEqual({ attach: [], detach: [] });
  expect(addLink({ attach: [], detach: [] }, "go", new Set())).toEqual({ attach: ["go"], detach: [] });
  expect(linksValue({ attach: ["sql", "go"], detach: ["sql"] })).toBe(JSON.stringify({ attach: ["go"], detach: ["sql"] }));
  expect(linksValue({ attach: [], detach: [] })).toBe("");
});

it("lays resource permissions out as a matrix and keeps what the user cannot change", async () => {
  const saved: unknown[] = [];
  server.use(
    http.get("*/admin/api/system/permissions", () =>
      HttpResponse.json([
        { name: "posts.view", group: "posts", groupLabel: "Posts", grantable: true, area: "resources", areaLabel: "Resources", areaSort: 0, resource: "posts", ability: "view" },
        { name: "posts.update", group: "posts", groupLabel: "Posts", grantable: false, area: "resources", areaLabel: "Resources", areaSort: 0, resource: "posts", ability: "update" },
        { name: "posts.attach_tags", group: "posts", groupLabel: "Posts", grantable: true, area: "resources", areaLabel: "Resources", areaSort: 0, resource: "posts", ability: "attach_tags" },
        { name: "tags.view", group: "tags", groupLabel: "Tags", grantable: true, area: "resources", areaLabel: "Resources", areaSort: 0, resource: "tags", ability: "view" },
        { name: "settings.general.view", group: "settings", groupLabel: "Settings", grantable: true, area: "resources", areaLabel: "Resources", areaSort: 0 },
      ]),
    ),
    http.post("*/admin/api/system/roles", async ({ request }) => {
      saved.push(await request.json());
      return HttpResponse.json({ id: "9", name: "x", isSystem: false, isPrivileged: false, permissions: [], users: 0, abilities: { update: true, delete: true, assign: true } }, { status: 201 });
    }),
  );
  mount("/system/roles/create");
  expect(await screen.findByRole("checkbox", { name: "Posts: View" })).toBeInTheDocument();
  expect(screen.getByRole("checkbox", { name: "Posts: Update" })).toBeDisabled();
  expect(screen.getByRole("checkbox", { name: "Posts: attach_tags" })).toBeInTheDocument();
  // Outside the matrix: by group.
  expect(screen.getByRole("group", { name: "Settings" })).toContainElement(screen.getByRole("checkbox", { name: "settings.general.view" }));
  // A whole column: only what the user may grant.
  await userEvent.click(screen.getByRole("checkbox", { name: "View on every resource" }));
  expect(screen.getByRole("checkbox", { name: "Posts: View" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Tags: View" })).toBeChecked();
  // A whole row skips the update it may not grant.
  await userEvent.click(screen.getByRole("checkbox", { name: "Every permission of Posts" }));
  expect(screen.getByRole("checkbox", { name: "Posts: attach_tags" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Posts: Update" })).not.toBeChecked();
  // The row reads as switched on (what may be granted is), and switches off again.
  const row = screen.getByRole("checkbox", { name: "Every permission of Posts" });
  expect(row).toBeChecked();
  await userEvent.click(row);
  expect(screen.getByRole("checkbox", { name: "Posts: View" })).not.toBeChecked();
});

it("saving a role keeps permissions the user cannot change, with what it started from", async () => {
  const puts: unknown[] = [];
  const role = { id: "4", name: "editor", isSystem: false, isPrivileged: false, permissions: ["posts.update"], users: 1, abilities: { update: true, delete: true, assign: true } };
  server.use(
    http.get("*/admin/api/system/permissions", () =>
      HttpResponse.json([
        { name: "posts.view", group: "posts", groupLabel: "Posts", grantable: true, area: "resources", areaLabel: "Resources", areaSort: 0, resource: "posts", ability: "view" },
        { name: "posts.update", group: "posts", groupLabel: "Posts", grantable: false, area: "resources", areaLabel: "Resources", areaSort: 0, resource: "posts", ability: "update" },
      ]),
    ),
    http.get("*/admin/api/system/roles/4", () => HttpResponse.json(role)),
    http.get("*/admin/api/system/roles", () => HttpResponse.json([role])),
    http.put("*/admin/api/system/roles/4/permissions", async ({ request }) => {
      puts.push(await request.json());
      return HttpResponse.json({ ...role, permissions: ["posts.update", "posts.view"] });
    }),
  );
  mount("/system/roles/4");
  const update = await screen.findByRole("checkbox", { name: "Posts: Update" });
  expect(update).toBeChecked();
  expect(update).toBeDisabled();
  await userEvent.click(screen.getByRole("checkbox", { name: "Every permission of Posts" }));
  await userEvent.click(screen.getByRole("button", { name: "Save permissions" }));
  await waitFor(() => expect(puts).toHaveLength(1));
  expect(puts[0]).toEqual({ permissions: ["posts.update", "posts.view"], expectedPermissions: ["posts.update"] });
});
