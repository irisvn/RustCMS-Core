import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { act, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { StrictMode, useEffect } from "react";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { HOST_VERSIONS, incompatibility } from "@/lib/host";
import { canonicalSubPath, extensionPath, extensionUrl } from "@/lib/paths";
import { queryPairs } from "@/lib/facade";
import { loadExtensions, registerPage, registerReactView, resetWidgets } from "@/lib/widgets";
// The SDK as an extension bundles it (its `react` and React Query are the panel's here).
import { adminKey, useAdminMutation } from "../sdk/index.js";

const panel = {
  brand: "T",
  user: { id: "1", name: "A", email: "a@x.y" },
  navigation: [],
  system: { users: false, roles: false, createUsers: false, createRoles: false, audit: false, queue: false },
  settings: [],
  pages: [{ slug: "react", label: "React" }],
  permissions: ["docker.view"],
};

const pageRequests: string[] = [];
const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json(panel)),
  http.get("*/admin/api/pages/react*", ({ request }) => {
    pageRequests.push(new URL(request.url).pathname);
    return HttpResponse.json({ slug: "react", title: "React", content: { kind: "script", script: "test/react", data: { n: 1 } } });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => {
  server.resetHandlers();
  resetWidgets();
  pageRequests.length = 0;
});

function mount(path: string, { strict = false } = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "/admin", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: [path] }));
  const tree = (
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>
  );
  const { unmount } = render(strict ? <StrictMode>{tree}</StrictMode> : tree);
  return { router, queryClient, session, unmount };
}

describe("extension paths", () => {
  it("accept one leading slash and plain or once-encoded segments", () => {
    expect(extensionPath("/docker/containers/web")).toBe("/docker/containers/web");
    expect(extensionPath("/a%2Db")).toBe("/a-b");
    expect(extensionUrl("/admin/api", "/docker/x")).toBe("/admin/api/x/docker/x");
    expect(extensionUrl("/back/api", "/a")).toBe("/back/api/x/a");
  });

  it("refuse traps", () => {
    for (const bad of ["docker", "//evil", "http://evil", "/a/../b", "/%2e%2e", "/%2E.", "/%252e", "/a%2Fb", "/a\\b", "/a?b", "/a#b", "/a//b", "/a%zz", "/a\u0001"]) {
      expect(() => extensionPath(bad), bad).toThrow();
    }
  });

  it("sub-paths follow the server's rules", () => {
    expect(canonicalSubPath("")).toBe("");
    expect(canonicalSubPath("containers/web/")).toBe("containers/web");
    for (const bad of ["a%2Fb", "%2e%2e", "%252e", "a//b", "a/".repeat(17)]) expect(canonicalSubPath(bad), bad).toBeNull();
  });

  it("query pairs are sorted, keep duplicates, and differ for different queries", () => {
    expect(queryPairs({ b: "2", a: ["1", "3"] })).toEqual([["a", "1"], ["a", "3"], ["b", "2"]]);
    expect(JSON.stringify(queryPairs({ a: "1" }))).not.toBe(JSON.stringify(queryPairs({ a: "2" })));
  });
});

describe("compatibility", () => {
  const ok = { api: 2, react: HOST_VERSIONS.react.split(".").slice(0, 2).join("."), reactQuery: "5.0", ui: 1 };
  it("needs the same API and ui, the same majors and no newer minors", () => {
    expect(incompatibility(ok)).toBeNull();
    expect(incompatibility({ ...ok, api: 3 })).toMatch(/host API/);
    expect(incompatibility({ ...ok, ui: 2 })).toMatch(/ui/);
    expect(incompatibility({ ...ok, react: "18.3" })).toMatch(/React/);
    expect(incompatibility({ ...ok, reactQuery: "5.999" })).toMatch(/React Query/);
  });

  it("an incompatible script never runs, and v1 still sees apiVersion 1", async () => {
    const inserted: string[] = [];
    const append = document.head.append.bind(document.head);
    const spy = vi.spyOn(document.head, "append").mockImplementation((...nodes) => {
      for (const node of nodes) {
        if (node instanceof HTMLScriptElement) {
          inserted.push(node.getAttribute("src") ?? "");
          queueMicrotask(() => node.onload?.(new Event("load")));
        }
      }
      return append(...nodes.filter((n) => !(n instanceof HTMLScriptElement)));
    });
    vi.spyOn(console, "error").mockImplementation(() => {});
    await loadExtensions("/admin", {
      scripts: ["assets/ext/old.0123abcd.js", "assets/ext/new.0123abcd.js"],
      styles: [],
      v2: { "assets/ext/new.0123abcd.js": { ...ok, react: "99.0" } },
    });
    spy.mockRestore();
    expect(inserted).toEqual(["/admin/assets/ext/old.0123abcd.js"]);
    const host = (window as unknown as { rustcms: { apiVersion: number; v2: { apiVersion: number } } }).rustcms;
    expect(host.apiVersion).toBe(1);
    expect(host.v2.apiVersion).toBe(2);
  });

  it("one name is taken once, by either version", () => {
    registerPage("test/one", { apiVersion: 1, mount: () => ({ update() {}, unmount() {} }) });
    expect(() => registerReactView("page", "test/one", () => null)).toThrow(/already registered/);
    registerReactView("page", "test/two", () => null);
    expect(() => registerPage("test/two", { apiVersion: 1, mount: () => ({ update() {}, unmount() {} }) })).toThrow(/already registered/);
  });
});

type PageProps = {
  realtime: { events: (path: string, handlers: object) => { close(): void } };
  data: { n: number };
  path: string;
  search: URLSearchParams;
  scope: { namespace: string; epoch: number };
  can: (p: string) => boolean;
  api: { post: <T>(path: string, body?: unknown) => Promise<T> };
};

describe("v2 pages", () => {
  it("render inside the host tree with the shared QueryClient, their sub-path and search", async () => {
    let seen: PageProps | undefined;
    function Page(props: PageProps) {
      seen = props;
      const q = useQuery({ queryKey: ["ext", "probe"], queryFn: () => Promise.resolve("shared") });
      return <p>React page {props.data.n} {props.path} {q.data}</p>;
    }
    registerReactView("page", "test/react", Page as never);
    const { queryClient } = mount("/p/react/items/2?tab=logs");
    expect(await screen.findByText("React page 1 items/2 shared")).toBeInTheDocument();
    expect(queryClient.getQueryData(["ext", "probe"])).toBe("shared");
    expect(pageRequests.at(-1)).toBe("/admin/api/pages/react/items/2");
    expect(seen!.search.get("tab")).toBe("logs");
    expect(seen!.scope.namespace).toBe("page:react");
    expect(Object.isFrozen(seen!.data)).toBe(true);
    await waitFor(() => expect(seen!.can("docker.view")).toBe(true));
    expect(seen!.can("docker.manage")).toBe(false);
  });

  it("an invalid sub-path is not found, with no request", async () => {
    registerReactView("page", "test/react", (() => <p>never</p>) as never);
    mount("/p/react/a%2Fb");
    expect(await screen.findByText(/not found/i)).toBeInTheDocument();
    expect(pageRequests).toEqual([]);
  });

  it("api posts with CSRF, and a call after the session changed is refused", async () => {
    const posts: string[] = [];
    server.use(
      http.post("*/admin/api/x/docker/restart", ({ request }) => {
        posts.push(request.headers.get("x-csrf-token") ?? "");
        return HttpResponse.json({ ok: true });
      }),
    );
    let api: PageProps["api"] | undefined;
    function Page(props: PageProps) {
      useEffect(() => {
        api = props.api;
      });
      return <p>ready</p>;
    }
    registerReactView("page", "test/react", Page as never);
    const { session } = mount("/p/react");
    await screen.findByText("ready");
    await expect(api!.post("/docker/restart", {})).resolves.toEqual({ ok: true });
    expect(posts).toEqual(["t"]);
    const stale = api!;
    act(() => session.api.resetCsrf());
    await expect(stale.post("/docker/restart", {})).rejects.toMatchObject({ code: "session_changed" });
    expect(posts).toHaveLength(1);
    await expect(stale.post("/../auth/logout", {})).rejects.toThrow();
  });

  it("a throwing view shows Retry and leaves the page", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    let fail = true;
    function Page() {
      if (fail) throw new Error("boom");
      return <p>recovered</p>;
    }
    registerReactView("page", "test/react", Page as never);
    mount("/p/react");
    const retry = await screen.findByRole("button", { name: "Retry" });
    expect(screen.getByRole("heading", { name: "React" })).toBeInTheDocument();
    fail = false;
    act(() => retry.click());
    expect(await screen.findByText("recovered")).toBeInTheDocument();
  });

  it("a queued write of a view that went away is never sent", async () => {
    const posts: string[] = [];
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    server.use(
      http.post("*/admin/api/x/docker/:op", async ({ params }) => {
        posts.push(String(params.op));
        if (params.op === "first") await held;
        return HttpResponse.json({ ok: true });
      }),
    );
    let api: PageProps["api"] | undefined;
    function Page(props: PageProps) {
      useEffect(() => {
        api = props.api;
      });
      return <p>ready</p>;
    }
    registerReactView("page", "test/react", Page as never);
    const { unmount } = mount("/p/react");
    await screen.findByText("ready");
    const first = api!.post("/docker/first", {});
    const second = api!.post("/docker/second", {});
    await waitFor(() => expect(posts).toEqual(["first"]));
    unmount();
    release();
    await expect(first).rejects.toMatchObject({ code: "session_changed" });
    await expect(second).rejects.toMatchObject({ code: "session_changed" });
    expect(posts).toEqual(["first"]);
  });

  it("under StrictMode a stream stays open", async () => {
    const opened: AbortSignal[] = [];
    server.use(
      http.get("*/admin/api/x/demo/ticks", ({ request }) => {
        opened.push(request.signal);
        return new HttpResponse(new ReadableStream({ start() {} }), { headers: { "content-type": "text/event-stream" } });
      }),
    );
    function Page(props: PageProps) {
      useEffect(() => {
        const handle = props.realtime.events("/demo/ticks", {});
        return () => handle.close();
      }, [props.realtime]);
      return <p>streaming</p>;
    }
    registerReactView("page", "test/react", Page as never);
    const { unmount } = mount("/p/react", { strict: true });
    await screen.findByText("streaming");
    await waitFor(() => expect(opened.filter((s) => !s.aborted)).toHaveLength(1));
    unmount();
    await waitFor(() => expect(opened.filter((s) => !s.aborted)).toHaveLength(0));
  });

  it("an unknown write outcome shows the paused banner, and can() fails closed after a failed refresh", async () => {
    server.use(http.post("*/admin/api/x/docker/restart", () => new HttpResponse(null, { status: 502 })));
    let seen: PageProps | undefined;
    function Page(props: PageProps) {
      seen = props;
      return <p>ready</p>;
    }
    registerReactView("page", "test/react", Page as never);
    const { queryClient } = mount("/p/react");
    await screen.findByText("ready");
    await expect(seen!.api.post("/docker/restart", {})).rejects.toMatchObject({ code: "outcome_unknown" });
    expect(await screen.findByText(/reload the page and check/i)).toBeInTheDocument();
    await expect(seen!.api.post("/docker/restart", {})).rejects.toMatchObject({ code: "writes_paused" });
    await waitFor(() => expect(seen!.can("docker.view")).toBe(true));
    server.use(http.get("*/admin/api/panel", () => new HttpResponse(null, { status: 500 })));
    await act(() => queryClient.refetchQueries());
    await waitFor(() => expect(seen!.can("docker.view")).toBe(false));
  });
});

describe("SDK hooks", () => {
  it("cache keys use the path as sent", () => {
    const props = { scope: { namespace: "page:a", epoch: 1 }, locale: "en" };
    expect(adminKey(props as never, "/a%2Db")).toEqual(adminKey(props as never, "/a-b"));
  });

  it("no mutation callback runs after unmount", async () => {
    let resolve!: (v: unknown) => void;
    const props = {
      scope: { namespace: "page:a", epoch: 1 },
      locale: "en",
      api: { post: () => new Promise((r) => (resolve = r)) },
    };
    const settled = vi.fn();
    let mutate!: (body: unknown) => void;
    function View() {
      const m = useAdminMutation(props as never, "/x", "post", { onSettled: settled });
      mutate = m.mutate;
      return null;
    }
    const client = new QueryClient();
    const { unmount } = render(
      <QueryClientProvider client={client}>
        <View />
      </QueryClientProvider>,
    );
    act(() => mutate({}));
    await waitFor(() => expect(resolve).toBeTypeOf("function"));
    unmount();
    resolve({ ok: true });
    await new Promise((r) => setTimeout(r, 10));
    expect(settled).not.toHaveBeenCalled();
  });
});
