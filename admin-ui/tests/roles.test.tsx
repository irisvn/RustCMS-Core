// Roles page: areas, search, presets (P7).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { fold } from "@/pages/system/roles";

const ROLE = { id: "4", name: "ops", isSystem: false, isPrivileged: false, permissions: ["docker.remove"], users: 1, abilities: { update: true, delete: true, assign: true } };

const PERMISSIONS = [
  { name: "posts.view", group: "posts", groupLabel: "Posts", grantable: true, resource: "posts", ability: "view", area: "resources", areaLabel: "Resources", areaSort: 0 },
  { name: "posts.update", group: "posts", groupLabel: "Posts", grantable: true, resource: "posts", ability: "update", area: "resources", areaLabel: "Resources", areaSort: 0 },
  { name: "docker.view", group: "docker", groupLabel: "Docker", grantable: true, label: "See containers", description: "List containers and their logs", area: "docker", areaLabel: "Docker", areaSort: 20 },
  { name: "docker.manage", group: "docker", groupLabel: "Docker", grantable: true, label: "Điều khiển containers", area: "docker", areaLabel: "Docker", areaSort: 20 },
  { name: "docker.remove", group: "docker", groupLabel: "Docker", grantable: false, label: "Remove containers", area: "docker", areaLabel: "Docker", areaSort: 20 },
  { name: "docker.prune", group: "docker", groupLabel: "Docker", grantable: false, label: "Prune images", area: "docker", areaLabel: "Docker", areaSort: 20 },
  { name: "users.view_any", group: "users", groupLabel: "Users", grantable: true, area: "system", areaLabel: "System", areaSort: 1000 },
];

const PRESETS = [
  { id: "viewer", label: "Viewer", description: "Read-only", permissions: ["docker.view", "docker.prune", "posts.view"] },
];

const puts: unknown[] = [];
const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "5" }, guard: "admin", permissions: [], superAdmin: false })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "5" })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({ brand: "T", user: { id: "5", name: "C", email: "c@x.y" }, navigation: [], system: { users: false, roles: true } }),
  ),
  http.get("*/admin/api/system/permissions", () => HttpResponse.json(PERMISSIONS)),
  http.get("*/admin/api/system/permission-presets", () => HttpResponse.json(PRESETS)),
  http.get("*/admin/api/system/roles/4", () => HttpResponse.json(ROLE)),
  http.get("*/admin/api/system/roles", () => HttpResponse.json([ROLE])),
  http.put("*/admin/api/system/roles/4/permissions", async ({ request }) => {
    puts.push(await request.json());
    return HttpResponse.json(ROLE);
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => {
  server.resetHandlers();
  puts.length = 0;
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

// The area's disclosure button (the shell has buttons of the same name).
const area = (name: string) => {
  const found = screen.getAllByRole("button", { name: new RegExp(`^${name}`) }).filter((b) => b.hasAttribute("aria-controls"));
  expect(found).toHaveLength(1);
  return found[0]!;
};

it("folds case and Vietnamese diacritics, đ included", () => {
  expect(fold("Điều KHIỂN")).toBe("dieu khien");
  expect(fold("posts.view")).toBe("posts.view");
});

it("groups permissions by area, ordered, collapsible, with counts", async () => {
  mount("/system/roles/4");
  await screen.findByRole("button", { name: /^Resources/ });
  const titles = screen.getAllByRole("button").filter((b) => b.hasAttribute("aria-controls"));
  expect(titles.map((b) => b.textContent?.replace(/\d+\/\d+/, ""))).toEqual(["Resources", "Docker", "System"]);
  // Counts read the whole area: one of four selected in Docker.
  expect(within(area("Docker")).getByLabelText("1 of 4 selected")).toHaveTextContent("1/4");
  const docker = area("Docker");
  expect(docker).toHaveAttribute("aria-expanded", "true");
  await userEvent.click(docker);
  expect(docker).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("checkbox", { name: /See containers/ })).toBeNull();
  await userEvent.click(docker);
  // Labels first, the name below, the description describing the box.
  const see = screen.getByRole("checkbox", { name: /See containers/ });
  expect(see).toHaveAccessibleDescription(/List containers and their logs/);
  expect(screen.getByText("docker.view")).toBeInTheDocument();
});

it("search filters by label, name and description, and bulk toggles only change what is shown", async () => {
  mount("/system/roles/4");
  await screen.findByRole("button", { name: /^Docker/ });
  await userEvent.type(screen.getByRole("searchbox", { name: "Search permissions" }), "dieu");
  expect(screen.getByRole("checkbox", { name: /Điều khiển containers/ })).toBeInTheDocument();
  expect(screen.queryByRole("checkbox", { name: /See containers/ })).toBeNull();
  expect(screen.queryByRole("button", { name: /^Resources/ })).toBeNull();
  // The group toggle is offered only for more than one shown permission.
  expect(screen.queryByRole("checkbox", { name: "Every permission of Docker" })).toBeNull();
  await userEvent.clear(screen.getByRole("searchbox", { name: "Search permissions" }));
  await userEvent.type(screen.getByRole("searchbox", { name: "Search permissions" }), "containers");
  // See (by description), Điều khiển and Remove (by label) are shown; Prune is not.
  expect(screen.queryByRole("checkbox", { name: /Prune images/ })).toBeNull();
  await userEvent.click(screen.getByRole("checkbox", { name: "Every permission of Docker" }));
  expect(screen.getByRole("checkbox", { name: /See containers/ })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: /Điều khiển containers/ })).toBeChecked();
  // Not grantable: kept as it was.
  expect(screen.getByRole("checkbox", { name: /Remove containers/ })).toBeChecked();
  await userEvent.clear(screen.getByRole("searchbox", { name: "Search permissions" }));
  expect(screen.getByRole("checkbox", { name: /Prune images/ })).not.toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Posts: View" })).not.toBeChecked();
  await userEvent.type(screen.getByRole("searchbox", { name: "Search permissions" }), "zzz");
  expect(screen.getByText("No permissions match.")).toBeInTheDocument();
});

it("a preset sets exactly its grantable permissions, keeps the rest, and saves nothing by itself", async () => {
  mount("/system/roles/4");
  await screen.findByRole("button", { name: /^Docker/ });
  await userEvent.click(screen.getByRole("checkbox", { name: "Posts: Update" }));
  await userEvent.click(screen.getByRole("button", { name: "Start from preset" }));
  await userEvent.click(screen.getByRole("menuitem", { name: /Viewer/ }));
  expect(screen.getByRole("status")).toHaveTextContent("Preset “Viewer” applied: 2 added, 1 removed.");
  expect(screen.getByRole("status")).toHaveTextContent("1 permission of this preset cannot be granted by you");
  expect(screen.getByRole("checkbox", { name: "Posts: View" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Posts: Update" })).not.toBeChecked();
  expect(screen.getByRole("checkbox", { name: /See containers/ })).toBeChecked();
  // Not grantable: Remove stays selected, Prune stays unselected.
  expect(screen.getByRole("checkbox", { name: /Remove containers/ })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: /Prune images/ })).not.toBeChecked();
  expect(puts).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "Save permissions" }));
  await vi.waitFor(() => expect(puts).toHaveLength(1));
  expect(puts[0]).toEqual({ permissions: ["docker.remove", "docker.view", "posts.view"], expectedPermissions: ["docker.remove"] });
});

it("offers no preset when the role cannot be changed", async () => {
  server.use(http.get("*/admin/api/system/roles/4", () => HttpResponse.json({ ...ROLE, abilities: { update: false, delete: false, assign: false } })));
  mount("/system/roles/4");
  await screen.findByRole("button", { name: /^Docker/ });
  expect(screen.queryByRole("button", { name: "Start from preset" })).toBeNull();
});

it("with more than three areas only the first starts open", async () => {
  server.use(http.get("*/admin/api/system/permissions", () => HttpResponse.json([
    ...PERMISSIONS,
    { name: "apps.view", group: "apps", grantable: true, area: "apps", areaLabel: "Apps", areaSort: 10 },
  ])));
  mount("/system/roles/4");
  await screen.findByRole("button", { name: /^Docker/ });
  expect(area("Resources")).toHaveAttribute("aria-expanded", "true");
  for (const name of ["Apps", "Docker", "System"]) expect(area(name)).toHaveAttribute("aria-expanded", "false");
  // A search opens the areas with matches.
  await userEvent.type(screen.getByRole("searchbox", { name: "Search permissions" }), "containers");
  expect(area("Docker")).toHaveAttribute("aria-expanded", "true");
});

it("matrix rows and columns under a search change only shown, grantable cells, and the save keeps the rest", async () => {
  const role = { ...ROLE, permissions: ["tags.update"] };
  server.use(
    http.get("*/admin/api/system/roles/4", () => HttpResponse.json(role)),
    http.get("*/admin/api/system/permissions", () => HttpResponse.json([
      ...PERMISSIONS,
      { name: "tags.view", group: "tags", groupLabel: "Tags", grantable: true, resource: "tags", ability: "view", area: "resources", areaLabel: "Resources", areaSort: 0 },
      { name: "tags.update", group: "tags", groupLabel: "Tags", grantable: false, resource: "tags", ability: "update", area: "resources", areaLabel: "Resources", areaSort: 0 },
      { name: "tags.delete", group: "tags", groupLabel: "Tags", grantable: false, resource: "tags", ability: "delete", label: "Remove tags", description: "Deletes tags for good", area: "resources", areaLabel: "Resources", areaSort: 0 },
    ])),
  );
  mount("/system/roles/4");
  await screen.findByRole("button", { name: /^Docker/ });
  // A declared label names the cell; its description describes it.
  expect(screen.getByRole("checkbox", { name: "Tags: Remove tags" })).toHaveAccessibleDescription(/Deletes tags for good/);
  // "Tags" matches the tags row only: Posts is hidden.
  await userEvent.type(screen.getByRole("searchbox", { name: "Search permissions" }), "tags");
  expect(screen.queryByRole("checkbox", { name: "Posts: View" })).toBeNull();
  await userEvent.click(screen.getByRole("checkbox", { name: "View on every resource" }));
  expect(screen.getByRole("checkbox", { name: "Tags: View" })).toBeChecked();
  // The row reads as on (all it may grant is), and switches off what it may grant only.
  await userEvent.click(screen.getByRole("checkbox", { name: "Every permission of Tags" }));
  expect(screen.getByRole("checkbox", { name: "Tags: View" })).not.toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Tags: Update" })).toBeChecked();
  await userEvent.click(screen.getByRole("checkbox", { name: "Every permission of Tags" }));
  expect(screen.getByRole("checkbox", { name: "Tags: View" })).toBeChecked();
  // Not grantable: one selected, one not, both unchanged.
  expect(screen.getByRole("checkbox", { name: "Tags: Update" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Tags: Remove tags" })).not.toBeChecked();
  await userEvent.clear(screen.getByRole("searchbox", { name: "Search permissions" }));
  expect(screen.getByRole("checkbox", { name: "Posts: View" })).not.toBeChecked();
  await userEvent.click(screen.getByRole("button", { name: "Save permissions" }));
  await vi.waitFor(() => expect(puts).toHaveLength(1));
  expect(puts[0]).toEqual({ permissions: ["tags.update", "tags.view"], expectedPermissions: ["tags.update"] });
});

it("every disclosure controls an element, and a new search reopens what the last one closed", async () => {
  mount("/system/roles/4");
  await screen.findByRole("button", { name: /^Docker/ });
  await userEvent.click(area("Docker"));
  for (const button of screen.getAllByRole("button").filter((b) => b.hasAttribute("aria-controls"))) {
    expect(document.getElementById(button.getAttribute("aria-controls")!)).not.toBeNull();
  }
  await userEvent.click(area("Docker"));
  const search = screen.getByRole("searchbox", { name: "Search permissions" });
  await userEvent.type(search, "containers");
  await userEvent.click(area("Docker"));
  expect(area("Docker")).toHaveAttribute("aria-expanded", "false");
  await userEvent.clear(search);
  await userEvent.type(search, "containers");
  expect(area("Docker")).toHaveAttribute("aria-expanded", "true");
});
