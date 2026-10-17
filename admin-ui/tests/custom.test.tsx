import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { panelPath } from "@/lib/config";
import { applyTheme } from "@/lib/theme";
import { registerCell, registerPage, resetWidgets, type View, type ViewContext } from "@/lib/widgets";
import posts from "../../fixtures/contract/posts.schema.json";

const panel = {
  brand: "T",
  user: { id: "1", name: "A", email: "a@x.y" },
  navigation: [{ slug: "posts", label: "Post", pluralLabel: "Posts", keyKind: "i64", icon: "file-text" }],
  system: { users: false, roles: false, createUsers: false, createRoles: false, audit: false, queue: false },
  settings: [],
  pages: [
    { slug: "overview", label: "Seat overview", icon: "layers", group: "Reports" },
    { slug: "report", label: "Report" },
  ],
};

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json(panel)),
  http.get("*/admin/api/pages/overview", () =>
    HttpResponse.json({
      slug: "overview",
      title: "Overview",
      content: {
        kind: "html",
        html: '<section class="rc-card"><h2>Seats</h2><a href="/admin/r/posts">All posts</a> <a href="https://example.test/docs" rel="noopener noreferrer">Docs</a></section>',
      },
    }),
  ),
  http.get("*/admin/api/pages/report", () =>
    HttpResponse.json({ slug: "report", title: "Report", content: { kind: "script", script: "test/report", data: { total: 3 } } }),
  ),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => {
  server.resetHandlers();
  resetWidgets();
});

function mount(path: string) {
  return mountWith(path).router;
}

function mountWith(path: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "/admin", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: [path] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  return { router, queryClient, session };
}

it("lists custom pages in their groups and shows a template page", async () => {
  const router = mount("/p/overview");
  expect(await screen.findByRole("heading", { name: "Seats" })).toBeInTheDocument();
  const nav = screen.getAllByRole("navigation", { name: "Resources" })[0]!;
  expect(within(nav).getByText("Reports")).toBeInTheDocument();
  expect(within(nav).getByRole("link", { name: "Seat overview" }).getAttribute("href")).toMatch(/\/p\/overview$/);
  // A link inside the panel navigates without leaving the app.
  server.use(http.get("*/admin/api/resources/posts/schema", () => HttpResponse.json({ ...posts, operation: "list" })),
    http.get("*/admin/api/resources/posts/records", () => HttpResponse.json({ data: [], meta: { page: 1, perPage: 20, total: 0, lastPage: 1 } })));
  await userEvent.click(screen.getByRole("link", { name: "All posts" }));
  expect(router.state.location.pathname).toBe("/r/posts");
});

it("mounts a script page with its data, and unmounts it on leaving", async () => {
  const calls: string[] = [];
  const view: View = {
    apiVersion: 1,
    mount(element, ctx) {
      calls.push(`mount ${JSON.stringify(ctx.data)}`);
      element.textContent = `Total ${(ctx.data as { total: number }).total}`;
      return { update: () => calls.push("update"), unmount: () => calls.push("unmount") };
    },
  };
  registerPage("test/report", view);
  const router = mount("/p/report");
  expect(await screen.findByText("Total 3")).toBeInTheDocument();
  expect(calls[0]).toBe('mount {"total":3}');
  await router.navigate({ to: "/p/$slug", params: { slug: "overview" } });
  await screen.findByRole("heading", { name: "Seats" });
  expect(calls).toContain("unmount");
});

it("gives a script page server-sent events, closed when the view goes", async () => {
  let aborted = false;
  server.use(
    http.get("*/admin/api/x/live/ticks", ({ request }) => {
      request.signal.addEventListener("abort", () => {
        aborted = true;
      });
      const body = new ReadableStream({
        start(controller) {
          controller.enqueue(new TextEncoder().encode('event: tick\ndata: {"n":7}\n\n'));
        },
      });
      return new HttpResponse(body, { headers: { "Content-Type": "text/event-stream" } });
    }),
  );
  registerPage("test/report", {
    apiVersion: 1,
    mount(element, ctx) {
      ctx.realtime.events("/live/ticks", {
        onEvent: (name, data) => {
          element.textContent = `${name} ${(data as { n: number }).n}`;
        },
      });
      return { update() {}, unmount() {} };
    },
  });
  const router = mount("/p/report");
  expect(await screen.findByText("tick 7")).toBeInTheDocument();
  await router.navigate({ to: "/p/$slug", params: { slug: "overview" } });
  await screen.findByRole("heading", { name: "Seats" });
  await waitFor(() => expect(aborted).toBe(true));
});

it("says so when a page's script is missing", async () => {
  mount("/p/report");
  expect(await screen.findByText(/extension script is missing or failed/)).toBeInTheDocument();
});

it("drops a page and its script when a refetch is refused", async () => {
  const calls: string[] = [];
  registerPage("test/report", {
    apiVersion: 1,
    mount(element) {
      element.textContent = "Report body";
      return { update: () => {}, unmount: () => calls.push("unmount") };
    },
  });
  const { queryClient } = mountWith("/p/report");
  expect(await screen.findByText("Report body")).toBeInTheDocument();
  server.use(http.get("*/admin/api/pages/report", () => HttpResponse.json({ message: "Forbidden", code: "forbidden" }, { status: 403 })));
  await queryClient.refetchQueries({ queryKey: ["custom", "page", "report"] });
  await waitFor(() => expect(screen.queryByText("Report body")).not.toBeInTheDocument());
  expect(calls).toContain("unmount");
});

it("refuses a script that returns no {update, unmount}", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  registerPage("test/report", {
    apiVersion: 1,
    mount(element) {
      element.textContent = "Half drawn";
      return undefined as never;
    },
  });
  mount("/p/report");
  expect(await screen.findByText(/extension script is missing or failed/)).toBeInTheDocument();
  expect(screen.queryByText("Half drawn")).not.toBeInTheDocument();
  error.mockRestore();
});

it("ignores a script's callbacks from an earlier session epoch", async () => {
  let ctx: ViewContext | undefined;
  registerPage("test/report", {
    apiVersion: 1,
    mount(element, context) {
      ctx = context;
      element.textContent = "Report body";
      return { update: () => {}, unmount: () => {} };
    },
  });
  const { router, session } = mountWith("/p/report");
  await screen.findByText("Report body");
  const earlier = ctx!;
  // Credentials rotated (in this tab or another): the old mount is stale.
  session.rotated();
  earlier.navigate("/r/posts");
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(router.state.location.pathname).toBe("/p/report");
});

it("draws cells with registered scripts and falls back when they fail", async () => {
  const columns = posts.table.columns.map((c) => (c.name === "title" ? { ...c, cell: "test/upper" } : c));
  server.use(
    http.get("*/admin/api/resources/posts/schema", () => HttpResponse.json({ ...posts, operation: "list", table: { ...posts.table, columns } })),
    http.get("*/admin/api/resources/posts/records", () =>
      HttpResponse.json({
        data: [{ key: "1", title: "first", abilities: { view: true, update: true, delete: true }, data: { title: "first" } }],
        meta: { page: 1, perPage: 20, total: 1, lastPage: 1 },
      }),
    ),
  );
  registerCell("test/upper", {
    apiVersion: 1,
    mount(element, ctx) {
      element.textContent = String(ctx.data).toUpperCase();
      return { update: (next) => (element.textContent = String(next.data).toUpperCase()), unmount: () => {} };
    },
  });
  mount("/r/posts");
  expect(await screen.findByText("FIRST")).toBeInTheDocument();
});

it("shows a failing cell's value the default way", async () => {
  const columns = posts.table.columns.map((c) => (c.name === "title" ? { ...c, cell: "test/broken" } : c));
  server.use(
    http.get("*/admin/api/resources/posts/schema", () => HttpResponse.json({ ...posts, operation: "list", table: { ...posts.table, columns } })),
    http.get("*/admin/api/resources/posts/records", () =>
      HttpResponse.json({
        data: [{ key: "1", title: "first", abilities: { view: true, update: true, delete: true }, data: { title: "first" } }],
        meta: { page: 1, perPage: 20, total: 1, lastPage: 1 },
      }),
    ),
  );
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  registerCell("test/broken", { apiVersion: 1, mount: () => { throw new Error("boom"); } });
  mount("/r/posts");
  expect(await screen.findByText("first")).toBeInTheDocument();
  error.mockRestore();
});

it("keeps links inside the panel only", () => {
  const origin = "http://localhost";
  for (const [href, path] of [
    ["/admin/r/posts", "/r/posts"],
    ["/admin", "/"],
    ["/admin/p/overview?tab=1#top", "/p/overview?tab=1#top"],
    ["http://localhost/admin/r/x", "/r/x"],
    ["/admin-evil/x", null],
    ["/admin/api/queue", null],
    ["/admin/assets/x.js", null],
    ["//evil.test/admin", null],
    ["/\\evil.test", null],
    ["https://evil.test/admin/r/x", null],
    ["javascript:alert(1)", null],
    [" /admin/r/x", null],
  ] as const) {
    expect(panelPath(href, "/admin", origin), href).toBe(path);
  }
});

it("applies the theme's color scheme and favicon", () => {
  const stop = applyTheme(
    { basePath: "/admin", apiPath: "/admin/api", theme: { colorScheme: "dark", favicon: "assets/ext/favicon.0123abcd.png" } },
    document,
  );
  expect(document.documentElement.dataset.scheme).toBe("dark");
  expect(document.head.querySelector('link[rel="icon"]')).toHaveAttribute("href", "/admin/assets/ext/favicon.0123abcd.png");
  stop();
});
