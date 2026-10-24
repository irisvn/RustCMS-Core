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
import { openableUrl } from "@/lib/open-url";
import { columnClass } from "@/renderers/table";
import posts from "../../fixtures/contract/posts.schema.json";
import outcomeContract from "../../fixtures/contract/action-outcome.json";

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
      row("a", { touch: true, unpublish: true, archive: true, publish: true }),
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

it("shows one labelled action per row and the others (bulk ones too) in a menu, destructive ones last", async () => {
  mount();
  await screen.findByText("Post a");
  const a = rowOf("Post a");
  expect(within(a).getByRole("button", { name: "Touch" })).toBeInTheDocument();
  expect(within(a).queryByRole("button", { name: "Archive" })).toBeNull();
  await userEvent.click(within(a).getByRole("button", { name: "More actions for Post a" }));
  const menu = await screen.findByRole("menu");
  expect([...menu.children].map((c) => c.getAttribute("role") === "separator" ? "---" : c.textContent)).toEqual(["Publish", "Archive", "---", "Unpublish"]);
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

describe("action outcomes", () => {
  it("open their page in a new tab without access back, and show the handler's message", async () => {
    const opened: unknown[][] = [];
    vi.stubGlobal("open", (...args: unknown[]) => {
      opened.push(args);
      return null;
    });
    onTestFinished(() => {
      vi.unstubAllGlobals();
    });
    server.use(http.post("*/admin/api/resources/posts/actions/:action", () =>
      HttpResponse.json({ ...outcomeContract, processed: ["a"], message: "Report ready." })));
    mount();
    await screen.findByText("Post a");
    await userEvent.click(within(rowOf("Post a")).getByRole("button", { name: "Touch" }));
    expect(await screen.findByText(/^Report ready\./)).toBeInTheDocument();
    expect(opened).toEqual([[outcomeContract.open, "_blank", "noopener,noreferrer"]]);
    const link = screen.getByRole("link", { name: "Open the page" });
    expect(link).toHaveAttribute("href", outcomeContract.open);
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
    server.resetHandlers();
  });

  it("never open what the server should have refused", async () => {
    const opened: unknown[] = [];
    vi.stubGlobal("open", (...args: unknown[]) => opened.push(args));
    onTestFinished(() => {
      vi.unstubAllGlobals();
    });
    server.use(http.post("*/admin/api/resources/posts/actions/:action", () =>
      HttpResponse.json({ processed: ["a"], denied: [], missing: [], open: "javascript:alert(1)" })));
    mount();
    await screen.findByText("Post a");
    await userEvent.click(within(rowOf("Post a")).getByRole("button", { name: "Touch" }));
    expect(await screen.findByText(/Touch: 1 record done/)).toBeInTheDocument();
    expect(opened).toEqual([]);
    expect(screen.queryByRole("link", { name: "Open the page" })).toBeNull();
    server.resetHandlers();
  });
});

describe("openable URLs", () => {
  it("follow the server's rules", () => {
    for (const ok of ["https://example.com/x?y#z", "HTTPS://example.com", "/admin", "/admin/r/posts?page=2", "/blog/a", "https://example.com:8443/x", "https://[::1]:8443/x", "https://127.0.0.1/x"]) {
      expect(openableUrl(ok), ok).toBe(true);
    }
    for (const bad of ["http://example.com", "javascript:alert(1)", "//evil", "https://u:p@evil", "https://", "/admin/../x", "/admin/%2e%2e/x", "relative", "https://a b", "https://a\\b", "", "data:text/html,x", `https://a.b/${"x".repeat(2048)}`, "https://example.com:abc/", "https://example.com:99999/", "https://[]/", "https://[/", "https://:", "https://a..b/", "https://-a.example/"]) {
      expect(openableUrl(bad), bad).toBe(false);
    }
  });
});

it("hides columns below their breakpoint with CSS only", async () => {
  mount();
  await screen.findByText("Post a");
  const header = (name: string) => screen.getByRole("columnheader", { name: new RegExp(`^${name}`) });
  expect(header("Category").className).toMatch(/(^| )hidden sm:table-cell( |$)/);
  expect(header("Published at").className).toMatch(/(^| )hidden md:table-cell( |$)/);
  expect(header("Title").className).not.toMatch(/hidden/);
  // Cells follow their header.
  const cells = rowOf("Post a").querySelectorAll("td");
  const index = [...screen.getAllByRole("columnheader")].findIndex((h) => h.textContent?.startsWith("Published at"));
  expect(cells[index]!.className).toContain("md:table-cell");
});

it("maps every breakpoint to a static class", () => {
  expect(["sm", "md", "lg"].map((b) => columnClass({ hideBelow: b as "sm" }))).toEqual(["hidden sm:table-cell", "hidden md:table-cell", "hidden lg:table-cell"]);
  expect(columnClass({})).toBe("");
});
