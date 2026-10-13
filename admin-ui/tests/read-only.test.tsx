import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { ComponentSchema, InputSchema } from "@/api/types";
import { createSession } from "@/app/session";
import { ReadOnlyComponents } from "@/renderers/form";

const requests: URL[] = [];
const server = setupServer(http.get("*/admin/api/resources/:slug/fields/:field/options", ({ request }) => {
  requests.push(new URL(request.url));
  return HttpResponse.json({ data: [{ value: "9007199254740993", label: "Authorized category" }] });
}));
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => { requests.length = 0; });
afterEach(() => server.resetHandlers());

const input = (name: string, overrides: Partial<InputSchema> = {}): InputSchema => ({
  name, label: name, type: "text_input", valueType: "string", nullable: true,
  disabled: false, rules: [], columnSpan: 1, ...overrides,
});

function mount(components: ComponentSchema[], data: Record<string, unknown>) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "/admin", apiPath: "http://localhost/admin/api" }, client, () => {});
  return render(<SessionContext.Provider value={session}><QueryClientProvider client={client}>
    <ReadOnlyComponents components={components} data={data} slug="posts" record="record-7" />
  </QueryClientProvider></SessionContext.Provider>);
}

it("shows only view-declared readable fields and distinguishes absent, null and empty without defaults", () => {
  const { container } = mount([
    input("missing", { default: "Not a stored value" }), input("nullable"), input("empty"),
    input("secret", { writeOnly: true }), input("hidden", { type: "hidden" }),
  ], { nullable: null, empty: "", secret: "private value", hidden: "hidden value", undeclared: "extra value" });
  expect(screen.getByText("Not provided")).toBeInTheDocument();
  expect(screen.getByText("Not set (null)")).toBeInTheDocument();
  expect(screen.getByText("Empty string")).toBeInTheDocument();
  for (const text of ["Not a stored value", "secret", "private value", "hidden", "hidden value", "extra value"]) {
    expect(screen.queryByText(text)).toBeNull();
  }
  expect(container.querySelectorAll("dt")).toHaveLength(3);
  expect(container.querySelectorAll("dd.select-text")).toHaveLength(3);
  expect(container.querySelector("input, select, textarea, button")).toBeNull();
});

it("preserves exact wire strings, full timestamps, boolean meaning, static labels and unknown values", () => {
  const unknown = { ...input("future"), type: "future_editor" } as unknown as InputSchema;
  const { container } = mount([
    input("integer", { type: "number_input", valueType: "i64" }),
    input("decimal", { type: "number_input", valueType: "decimal" }),
    input("instant", { type: "date_time_picker", valueType: "instant" }),
    input("enabled", { type: "toggle", valueType: "bool" }),
    input("status", { type: "select", options: [{ value: "live", label: "Published" }] }),
    input("tags", { type: "checkbox_list", options: [{ value: "one", label: "First" }] }), unknown,
  ], { integer: "9007199254740993", decimal: "10.50", instant: "2026-10-12T09:30:00.123456Z", enabled: false, status: "live", tags: ["one", "other"], future: { raw: "unknown" } });
  for (const text of ["9007199254740993", "10.50", "2026-10-12T09:30:00.123456Z", "No", "Published", "First", "other"]) {
    expect(screen.getByText(text)).toBeInTheDocument();
  }
  expect(container.querySelector("[data-fallback]")?.textContent).toBe(JSON.stringify({ raw: "unknown" }, null, 2));
});

it("looks up only declared relationship keys with record scope and never searches", async () => {
  mount([
    input("category", { type: "select", relationship: { resource: "categories" } }),
    input("missing", { type: "select", relationship: { resource: "categories" }, default: "2" }),
    input("private", { type: "select", relationship: { resource: "categories" }, writeOnly: true }),
    input("hidden", { type: "hidden", relationship: { resource: "categories" } }),
  ], { category: "9007199254740993", private: "3", hidden: "4", undeclared: "5" });
  expect(await screen.findByText("Authorized category")).toBeInTheDocument();
  expect(requests).toHaveLength(1);
  expect(requests[0]!.pathname).toBe("/admin/api/resources/posts/fields/category/options");
  expect(requests[0]!.searchParams.get("keys")).toBe("9007199254740993");
  expect(requests[0]!.searchParams.get("record")).toBe("record-7");
  expect(requests[0]!.searchParams.has("search")).toBe(false);
});

it("keeps the raw key selectable and makes a failed label lookup explicit", async () => {
  server.use(http.get("*/admin/api/resources/:slug/fields/:field/options", () => HttpResponse.json({ message: "Forbidden", code: "forbidden" }, { status: 403 })));
  mount([input("category", { type: "select", relationship: { resource: "categories" } })], { category: "7" });
  expect(screen.getByText("#7")).toBeInTheDocument();
  expect(await screen.findByText("Label unavailable")).toBeInTheDocument();
  await waitFor(() => expect(screen.queryByText("Loading label…")).toBeNull());
});

it("labels static boolean options and never shows password-mode values", () => {
  mount([
    input("featured", { type: "select", valueType: "bool", options: [{ value: true, label: "Featured" }, { value: false, label: "Regular" }] }),
    input("pin", { mode: "password" }),
  ], { featured: true, pin: "1234" });
  expect(screen.getByText("Featured")).toBeInTheDocument();
  expect(screen.queryByText("Yes")).toBeNull();
  expect(screen.queryByText("1234")).toBeNull();
  expect(screen.getByLabelText("Hidden value")).toBeInTheDocument();
});
