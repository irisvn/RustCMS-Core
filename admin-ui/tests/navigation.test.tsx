import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { PageLink, ResourceInfo } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { groups } from "@/pages/shell";

const user = { id: "1", name: "Admin", email: "admin@example.test" };
const r = (slug: string, extra: Partial<ResourceInfo> = {}): ResourceInfo => ({ slug, label: slug, pluralLabel: slug, ...extra }) as ResourceInfo;
const p = (slug: string, extra: Partial<PageLink> = {}): PageLink => ({ slug, label: slug, ...extra });
const names = (out: ReturnType<typeof groups>) => out.map(([group, items]) => [group, items.map((e) => e.item.slug)]);

describe("navigation order", () => {
  it("is today's order when nothing is set", () => {
    // The server lists resources by slug; pages come in registration order.
    const out = groups([r("ports", { group: "Docker" }), r("posts"), r("projects", { group: "Docker" })], [p("zeta"), p("alpha", { group: "Docker" })]);
    expect(names(out)).toEqual([["Docker", ["ports", "projects", "alpha"]], ["", ["posts", "zeta"]]]);
  });

  it("orders entries by sort and groups by their smallest sort, stably", () => {
    const out = groups(
      [r("ports", { group: "Docker", sort: 11 }), r("projects", { group: "Docker", sort: 10 }), r("services", { group: "Docker", sort: 10 })],
      [p("templates", { sort: 1 }), p("settings", { sort: 3 }), p("monitor", { sort: 2 })],
    );
    expect(names(out)).toEqual([["", ["templates", "monitor", "settings"]], ["Docker", ["projects", "services", "ports"]]]);
  });

  it("leaves out hidden resources and the home, which rank nothing", () => {
    const out = groups([r("deployments", { group: "Docker", nav: false, sort: -5 }), r("projects", { group: "Docker", sort: 10 })], [p("dashboard", { sort: -9 }), p("templates", { sort: 1 })], "dashboard");
    expect(names(out)).toEqual([["", ["templates"]], ["Docker", ["projects"]]]);
  });
});

const dashboardRequests: string[] = [];
function serve(panel: object) {
  return setupServer(
    http.get("*/api/auth/me", () => HttpResponse.json({ user, guard: "admin", permissions: [], superAdmin: false })),
    http.get("*/api/panel", () => HttpResponse.json(panel)),
    http.get("*/api/dashboard", () => {
      dashboardRequests.push("dashboard");
      return HttpResponse.json({ widgets: [], resourceLinks: true });
    }),
    http.get("*/api/pages/:slug*", ({ params }) =>
      HttpResponse.json({ slug: params.slug, title: "Home page", content: { kind: "html", html: "<p>Welcome home</p>" } }),
    ),
  );
}

const base = { brand: "W", user, system: { users: false, roles: false }, settings: [] };
const server = serve({});
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => {
  server.resetHandlers();
  dashboardRequests.length = 0;
});
afterAll(() => server.close());

function mount(path: string, panel: object) {
  server.use(http.get("*/api/panel", () => HttpResponse.json(panel)));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: [path] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  return { router };
}

describe("home and hidden resources", () => {
  const pages = [p("home", { label: "Control", icon: "layers" }), p("templates", { label: "Templates" })];

  it("the index opens the home page, with no dashboard request", async () => {
    const { router } = mount("/", { ...base, navigation: [], pages, home: "home" });
    expect(await screen.findByText("Welcome home")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/p/home");
    expect(dashboardRequests).toEqual([]);
    const nav = screen.getByRole("navigation", { name: "Resources" });
    // The first entry is the home page, listed once.
    expect(within(nav).getAllByRole("link", { name: "Control" })).toHaveLength(1);
    expect(within(nav).queryByRole("link", { name: "Overview" })).toBeNull();
  });

  it("without a home (or one the user may not open) the dashboard shows", async () => {
    mount("/", { ...base, navigation: [], pages });
    expect(await screen.findByRole("heading", { name: "Welcome, Admin" })).toBeInTheDocument();
    await waitFor(() => expect(dashboardRequests).toEqual(["dashboard"]));
  });

  it("a hidden resource has no sidebar link and no dashboard card", async () => {
    const navigation = [r("posts", { pluralLabel: "Posts" }), r("deployments", { pluralLabel: "Deployments", nav: false })];
    mount("/", { ...base, navigation, pages });
    await screen.findByRole("heading", { name: "Welcome, Admin" });
    expect(screen.queryByRole("link", { name: /Deployments/ })).toBeNull();
    expect(screen.getAllByRole("link", { name: /Posts/ }).length).toBeGreaterThan(0);
  });

  it("the breadcrumb names a page on its sub-paths", async () => {
    mount("/p/templates/a/b", { ...base, navigation: [], pages });
    const crumb = await screen.findByRole("navigation", { name: "Breadcrumb" });
    expect(within(crumb).getByText("Templates")).toBeInTheDocument();
  });
});
