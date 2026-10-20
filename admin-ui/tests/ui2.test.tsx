import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { readFileSync } from "node:fs";
import type * as React from "react";
import { useEffect, useState } from "react";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { Dialog } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuItem, DropdownMenuSeparator, Tooltip } from "@/components/ui/menu";
import { CodeBlock } from "@/components/ui/misc";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/lib/../components/ui/tabs";
import { MODULES } from "@/lib/host";
import { recordKey, resourcePath } from "@/lib/paths";
import { loadExtensions, reactViewLevel, registerReactView, resetWidgets } from "@/lib/widgets";
import { fieldErrors, registerPage as sdkRegisterPage, resourceKey } from "../sdk/index.js";

const panel = {
  brand: "T",
  user: { id: "1", name: "A", email: "a@x.y" },
  navigation: [],
  system: { users: false, roles: false },
  settings: [],
  pages: [{ slug: "react", label: "React" }],
  permissions: [],
};

type Sent = { method: string; path: string; search: string; key: string | null; body: unknown };
const sent: Sent[] = [];
const record = (slug: string, key: string) => ({ key, abilities: {}, data: { title: `${slug} ${key}` } });
const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json(panel)),
  http.get("*/admin/api/pages/react*", () =>
    HttpResponse.json({ slug: "react", title: "React", content: { kind: "script", script: "test/react", data: {} } }),
  ),
  http.all("*/admin/api/resources/*", async ({ request }) => {
    const url = new URL(request.url);
    const body = request.method === "GET" || request.method === "DELETE" ? undefined : await request.json();
    sent.push({ method: request.method, path: url.pathname, search: url.search, key: request.headers.get("idempotency-key"), body });
    if (request.method === "DELETE") return new HttpResponse(null, { status: 204 });
    if (url.pathname.endsWith("/records") && request.method === "GET") {
      return HttpResponse.json({ data: [record("posts", "1")], meta: { page: 1, perPage: 25, total: 1, lastPage: 1 } });
    }
    if (url.pathname.includes("/actions/")) return HttpResponse.json({ message: "done" });
    return HttpResponse.json({ data: record("posts", "1") });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => {
  server.resetHandlers();
  resetWidgets();
  sent.length = 0;
});

type Props = {
  resources?: {
    track(slug: string): void;
    list(slug: string, query?: object): Promise<unknown>;
    get(slug: string, key: string): Promise<unknown>;
    create(slug: string, data: object): Promise<unknown>;
    update(slug: string, key: string, data: object): Promise<unknown>;
    delete(slug: string, key: string): Promise<void>;
    restore(slug: string, key: string): Promise<unknown>;
    forceDelete(slug: string, key: string): Promise<void>;
    action(slug: string, name: string, keys: string[], data?: object): Promise<unknown>;
  };
  toast?: { success(text: string): void; error(text: string): void };
  scope: { namespace: string; epoch: number };
  locale: string;
};

let seen: Props | undefined;
function Probe(props: Props) {
  useEffect(() => {
    seen = props;
  });
  return <p>ready</p>;
}

function mount({ level = 2 }: { level?: number } = {}) {
  registerReactView("page", "test/react", Probe as never, { ui: level }, level);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "/admin", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/p/react"] }));
  const view = render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  return { session, queryClient, unmount: view.unmount };
}

describe("props.resources", () => {
  it("calls the resource routes as the panel's pages do", async () => {
    mount();
    await screen.findByText("ready");
    const r = seen!.resources!;
    await expect(r.list("posts", { status: "draft", a: ["2", "1"] })).resolves.toMatchObject({ meta: { total: 1 } });
    await expect(r.get("posts", "1")).resolves.toMatchObject({ key: "1" });
    await r.create("posts", { title: "x" });
    await r.update("posts", "1", { title: "y" });
    await r.delete("posts", "1");
    await r.restore("posts", "1");
    await r.forceDelete("posts", "1");
    await r.action("posts", "publish", ["1", "2"]);
    expect(sent.map((s) => `${s.method} ${s.path}${s.search}`)).toEqual([
      "GET /admin/api/resources/posts/records?a=2&a=1&status=draft",
      "GET /admin/api/resources/posts/records/1",
      "POST /admin/api/resources/posts/records",
      "PATCH /admin/api/resources/posts/records/1",
      "DELETE /admin/api/resources/posts/records/1",
      "POST /admin/api/resources/posts/records/1/restore",
      "DELETE /admin/api/resources/posts/records/1/force",
      "POST /admin/api/resources/posts/actions/publish",
    ]);
    // Creates and actions carry an idempotency key (a lost answer is retried safely).
    expect(sent.filter((s) => s.key).map((s) => s.method + s.path)).toEqual([
      "POST/admin/api/resources/posts/records",
      "POST/admin/api/resources/posts/actions/publish",
    ]);
    expect(sent.at(-1)!.body).toEqual({ keys: ["1", "2"], data: {} });
  });

  it("refuses names and keys a URL would change, before sending", async () => {
    mount();
    await screen.findByText("ready");
    const r = seen!.resources!;
    // `.` would make `/records/./force` the delete of the record `force`.
    for (const bad of [".", "..", "", "a\u0000"]) await expect(r.forceDelete("posts", bad)).rejects.toThrow(/record key/);
    await expect(r.get("Posts", "1")).rejects.toThrow(/resource/);
    await expect(r.action("posts", "../x", ["1"])).rejects.toThrow(/action/);
    expect(sent).toEqual([]);
    expect(() => recordKey("x".repeat(257))).toThrow();
    expect(resourcePath("/back/api", ["posts", "records", "a/b"])).toBe("/resources/posts/records/a%2Fb");
  });

  it("a 422 maps to field errors", async () => {
    server.use(
      http.post("*/admin/api/resources/posts/records", () =>
        HttpResponse.json({ code: "validation_failed", message: "invalid", errors: { title: ["is required"] } }, { status: 422 }),
      ),
    );
    mount();
    await screen.findByText("ready");
    const error = await seen!.resources!.create("posts", {}).catch((e: unknown) => e);
    expect(fieldErrors(error)).toEqual({ title: ["is required"] });
    expect(fieldErrors(new Error("x"))).toEqual({});
  });

  it("an unknown outcome pauses the resource for the built-in pages too, and the view shows it", async () => {
    server.use(http.patch("*/admin/api/resources/posts/records/1", () => new HttpResponse(null, { status: 502 })));
    const { session } = mount();
    await screen.findByText("ready");
    await expect(seen!.resources!.update("posts", "1", { title: "y" })).rejects.toMatchObject({ code: "outcome_unknown" });
    expect(session.isUncertain("posts")).toBe(true);
    expect(await screen.findByText(/reload the page and check/i)).toBeInTheDocument();
    await expect(seen!.resources!.update("posts", "1", {})).rejects.toMatchObject({ code: "writes_paused" });
  });

  it("a view opened on an already paused resource shows the banner before any write", async () => {
    function Tracking(props: Props) {
      useEffect(() => props.resources!.track("posts"), [props.resources]);
      return <p>ready</p>;
    }
    registerReactView("page", "test/react", Tracking as never, { ui: 2 }, 2);
    const queryClient = new QueryClient();
    const session = createSession({ basePath: "/admin", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
    session.identify("1");
    session.markUncertain("posts");
    const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/p/react"] }));
    render(
      <SessionContext.Provider value={session}>
        <QueryClientProvider client={queryClient}>
          <RouterProvider router={router} />
        </QueryClientProvider>
      </SessionContext.Provider>,
    );
    expect(await screen.findByText(/reload the page and check/i)).toBeInTheDocument();
    expect(sent).toEqual([]);
  });

  it("a queued write of a view that went away is never sent, and writes refresh resource reads", async () => {
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    server.use(
      http.post("*/admin/api/resources/posts/records", async () => {
        sent.push({ method: "POST", path: "first", search: "", key: null, body: null });
        await held;
        return HttpResponse.json({ data: record("posts", "1") });
      }),
    );
    const { queryClient, unmount } = mount();
    await screen.findByText("ready");
    queryClient.setQueryData(resourceKey(seen as never, "posts", "list", []), { data: [] });
    const r = seen!.resources!;
    const first = r.create("posts", {});
    const second = r.update("posts", "1", {});
    await waitFor(() => expect(sent.map((s) => s.path)).toEqual(["first"]));
    unmount();
    release();
    await expect(first).rejects.toMatchObject({ code: "session_changed" });
    await expect(second).rejects.toMatchObject({ code: "session_changed" });
    expect(sent.map((s) => s.path)).toEqual(["first"]);
    // The first was sent and succeeded: every view's reads of `posts` are stale.
    expect(queryClient.getQueryState(resourceKey(seen as never, "posts", "list", []))?.isInvalidated).toBe(true);
  });

  it("scripts that declare ui 1 get neither resources nor toast", async () => {
    mount({ level: 1 });
    await screen.findByText("ready");
    expect(seen!.resources).toBeUndefined();
    expect(seen!.toast).toBeUndefined();
  });
});

describe("levels", () => {
  it("a registration built with a newer SDK than its script declares is refused", () => {
    expect(() => registerReactView("page", "test/a", Probe as never, { ui: 2 }, 1)).toThrow(/update the provider/);
    registerReactView("page", "test/b", Probe as never, undefined, 1);
    registerReactView("page", "test/c", Probe as never, { ui: 1 }, 2);
  });

  it("the SDK refuses to register on an older panel", () => {
    const host = globalThis as unknown as { rustcms?: unknown };
    const before = host.rustcms;
    const register = vi.fn();
    host.rustcms = { v2: { versions: { ui: 1 }, registerPage: register } };
    expect(() => sdkRegisterPage("test/x", Probe)).toThrow(/newer panel/);
    host.rustcms = { v2: { versions: { ui: 2 }, registerPage: register } };
    sdkRegisterPage("test/x", Probe);
    expect(register).toHaveBeenCalledWith("test/x", Probe, { ui: 2 });
    host.rustcms = before;
  });

  it("the SDK's ui declarations match the panel's ui module", () => {
    const declared = [...readFileSync("sdk/ui.d.ts", "utf8").matchAll(/export declare (?:const|function) (\w+)/g)].map((m) => m[1]);
    expect(declared.sort()).toEqual(Object.keys(MODULES.ui).sort());
  });
});

describe("loading", () => {
  it("registrations inherit the Requires of the script running them", async () => {
    const append = document.head.append.bind(document.head);
    const host = () => (window as unknown as { rustcms: { v2: { registerPage: (n: string, c: unknown, m?: object) => void } } }).rustcms.v2;
    const errors: unknown[] = [];
    const spy = vi.spyOn(document.head, "append").mockImplementation((...nodes) => {
      for (const node of nodes) {
        if (node instanceof HTMLScriptElement) {
          queueMicrotask(() => {
            const current = vi.spyOn(document, "currentScript", "get").mockReturnValue(node);
            try {
              if (node.src.includes("v2")) host().registerPage("test/two", Probe, { ui: 2 });
              // A v1 script (no Requires) cannot claim level 2.
              else host().registerPage("test/one", Probe, { ui: 2 });
            } catch (e) {
              errors.push(e);
            } finally {
              current.mockRestore();
            }
            node.onload?.(new Event("load"));
          });
        }
      }
      return append(...nodes.filter((n) => !(n instanceof HTMLScriptElement)));
    });
    const requires = { api: 2, react: "19.0", reactQuery: "5.0", ui: 2 };
    await loadExtensions("/admin", { scripts: ["assets/ext/v2.0123abcd.js", "assets/ext/v1.0123abcd.js"], styles: [], v2: { "assets/ext/v2.0123abcd.js": requires } });
    spy.mockRestore();
    expect(reactViewLevel("page", "test/two")).toBe(2);
    expect(String(errors[0])).toMatch(/update the provider/);
  });
});

describe("recovery", () => {
  it("a lost create from a view is retried safely with the same key", async () => {
    let first = true;
    const keys: string[] = [];
    server.use(
      http.post("*/admin/api/resources/posts/records", ({ request }) => {
        keys.push(request.headers.get("idempotency-key") ?? "");
        if (first) {
          first = false;
          return new HttpResponse(null, { status: 502 });
        }
        return HttpResponse.json({ data: record("posts", "9") });
      }),
    );
    const { session } = mount();
    await screen.findByText("ready");
    session.identify("1");
    await expect(seen!.resources!.create("posts", { title: "x" })).rejects.toMatchObject({ code: "outcome_unknown" });
    await userEvent.click(await screen.findByRole("button", { name: "Retry safely" }));
    await waitFor(() => expect(keys).toHaveLength(2));
    expect(keys[0]).toBe(keys[1]);
    await waitFor(() => expect(session.isUncertain("posts")).toBe(false));
  });
});

describe("props.title", () => {
  function mountAt(path: string, View: (props: Props & { path: string; title?: (t: string | null) => void }) => React.ReactNode, level = 2) {
    registerReactView("page", "test/react", View as never, { ui: level }, level);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const session = createSession({ basePath: "/admin", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
    const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: [path] }));
    const view = render(
      <SessionContext.Provider value={session}>
        <QueryClientProvider client={queryClient}>
          <RouterProvider router={router} />
        </QueryClientProvider>
      </SessionContext.Provider>,
    );
    return { router, session, ...view };
  }

  it("names the sub-path in the heading and the breadcrumb, per sub-path, and hides the heading with null", async () => {
    function View(props: Props & { path: string; title?: (t: string | null) => void }) {
      useEffect(() => {
        if (props.path === "demo/web") props.title!("web");
        if (props.path === "bare") props.title!(null);
      }, [props.path, props.title]);
      return <p>at {props.path || "root"}</p>;
    }
    const { router } = mountAt("/p/react/demo/web", View);
    expect(await screen.findByRole("heading", { level: 1, name: "web" })).toBeVisible();
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("web");
    // Another sub-path: the server's title again until the view sets one.
    await act(() => router.navigate({ to: "/p/react" } as never));
    expect(await screen.findByText("at root")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "React" })).toBeInTheDocument();
    await act(() => router.navigate({ to: "/p/react/bare" } as never));
    await screen.findByText("at bare");
    // Hidden, still named for assistive technology.
    await waitFor(() => expect(screen.getByRole("heading", { level: 1, name: "React" })).toHaveClass("sr-only"));
  });

  it("is ui 2 only, and a title of an old session is ignored", async () => {
    let title: ((t: string | null) => void) | undefined;
    function View(props: Props & { path: string; title?: (t: string | null) => void }) {
      title = props.title;
      return <p>ready</p>;
    }
    const { session } = mountAt("/p/react", View);
    await screen.findByText("ready");
    const stale = title!;
    act(() => session.api.resetCsrf());
    act(() => stale("stale"));
    expect(screen.queryByRole("heading", { name: "stale" })).toBeNull();
    resetWidgets();
    cleanup();
    mountAt("/p/react", View, 1);
    await screen.findByText("ready");
    expect(title).toBeUndefined();
  });
});

describe("props.toast", () => {
  it("shows successes politely and errors assertively, at most five, and drops them with the session", async () => {
    const { session } = mount();
    await screen.findByText("ready");
    const toast = seen!.toast!;
    act(() => {
      toast.success("Saved");
      toast.error("Failed");
    });
    expect(screen.getByRole("status")).toHaveTextContent("Saved");
    expect(screen.getByRole("alert")).toHaveTextContent("Failed");
    act(() => {
      for (let i = 0; i < 7; i++) toast.success(`n${i}`);
    });
    expect(screen.queryByText("n1")).toBeNull();
    expect(screen.getAllByRole("status").map((s) => s.textContent)).toContain("n6");
    expect(screen.queryAllByRole("status").length + screen.queryAllByRole("alert").length).toBe(5);
    act(() => session.api.resetCsrf());
    await waitFor(() => expect(screen.queryByText("n6")).toBeNull());
    // The old view's toast is ignored in the new session.
    act(() => toast.error("stale"));
    expect(screen.queryByText("stale")).toBeNull();
  });
});

describe("ui 2 components", () => {
  it("tabs are controlled and move with the arrows", async () => {
    function T() {
      const [value, setValue] = useState("a");
      return (
        <Tabs value={value} onValueChange={setValue}>
          <TabsList aria-label="Sections">
            <TabsTrigger value="a">A</TabsTrigger>
            <TabsTrigger value="b">B</TabsTrigger>
          </TabsList>
          <TabsContent value="a">Panel A</TabsContent>
          <TabsContent value="b">Panel B</TabsContent>
        </Tabs>
      );
    }
    render(<T />);
    const a = screen.getByRole("tab", { name: "A" });
    expect(screen.getByRole("tabpanel")).toHaveAccessibleName("A");
    a.focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "B" })).toHaveFocus();
    expect(screen.getByRole("tab", { name: "B" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("Panel B")).toBeInTheDocument();
  });

  it("a menu opens, moves, selects, closes on Escape back to its button, and renders inside a dialog", async () => {
    const selected = vi.fn();
    render(
      <Dialog open onClose={() => {}} title="D">
        <DropdownMenu trigger="More" aria-label="More">
          <DropdownMenuItem onSelect={() => selected("edit")}>Edit</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem destructive onSelect={() => selected("delete")}>Delete</DropdownMenuItem>
        </DropdownMenu>
      </Dialog>,
    );
    const button = screen.getByRole("button", { name: "More" });
    await userEvent.click(button);
    const menu = screen.getByRole("menu");
    expect(menu.closest("dialog")).not.toBeNull();
    expect(screen.getByRole("menuitem", { name: "Edit" })).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "Delete" })).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(button).toHaveFocus();
    await userEvent.click(button);
    await userEvent.keyboard("{ArrowDown}{Enter}");
    expect(selected).toHaveBeenCalledWith("delete");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("Tab from a menu moves on from its button, and wraps inside a dialog", async () => {
    render(
      <Dialog open onClose={() => {}} title="D" closeButton={false}>
        <button type="button">Before</button>
        <DropdownMenu trigger="More" aria-label="More">
          <DropdownMenuItem>Edit</DropdownMenuItem>
        </DropdownMenu>
        <button type="button" tabIndex={-1}>Skipped</button>
      </Dialog>,
    );
    const more = screen.getByRole("button", { name: "More" });
    await userEvent.click(more);
    await userEvent.keyboard("{Tab}");
    expect(screen.queryByRole("menu")).toBeNull();
    // The last tabbable: Tab wraps to the first, as the dialog does.
    expect(screen.getByRole("button", { name: "Before" })).toHaveFocus();
    await userEvent.click(more);
    await userEvent.keyboard("{Shift>}{Tab}{/Shift}");
    expect(screen.getByRole("button", { name: "Before" })).toHaveFocus();
  });

  it("Escape on a tooltip is not the dialog's", async () => {
    render(
      <Tooltip content="Hint">
        <button type="button">Hover</button>
      </Tooltip>,
    );
    await userEvent.tab();
    await screen.findByRole("tooltip");
    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    act(() => {
      screen.getByRole("button", { name: "Hover" }).dispatchEvent(event);
    });
    expect(event.defaultPrevented).toBe(true);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("a tooltip shows on focus, describes its element, and closes on Escape", async () => {
    render(
      <Tooltip content="Restart the service">
        <button type="button">Restart</button>
      </Tooltip>,
    );
    await userEvent.tab();
    const tip = await screen.findByRole("tooltip");
    expect(screen.getByRole("button", { name: "Restart" })).toHaveAccessibleDescription("Restart the service");
    expect(tip).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("a code block copies its text, or says to copy by hand", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<CodeBlock code="docker ps" />);
    await userEvent.click(screen.getByRole("button", { name: "Copy" }));
    expect(writeText).toHaveBeenCalledWith("docker ps");
    expect(await screen.findByRole("button", { name: "Copied" })).toBeInTheDocument();
    Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
  });
});
