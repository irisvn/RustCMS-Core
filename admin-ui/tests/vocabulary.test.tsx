import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { InputSchema, ResourceSchema } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { fromFormValue, inputsOf, protectLossyJson, toFormValue, zodSchema } from "@/schema/form";
import create from "../../fixtures/contract/kitchen-sink.schema.create.json";

const schema = create as unknown as ResourceSchema;
const specs = inputsOf(schema.form.components).find((i) => i.name === "specs")!;
let creates: unknown[] = [];
let answer: () => Response = () => HttpResponse.json({ message: "x", code: "validation_failed", errors: {}, formErrors: ["stop"] }, { status: 422 });

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1", name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [schema.resource] })),
  http.get("*/admin/api/resources/gadgets/schema", () => HttpResponse.json(schema)),
  http.get("*/admin/api/resources/:slug/fields/:field/options", () => HttpResponse.json({ data: [] })),
  http.post("*/admin/api/resources/gadgets/records", async ({ request }) => {
    creates.push(await request.json());
    return answer();
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  creates = [];
  answer = () => HttpResponse.json({ message: "x", code: "validation_failed", errors: {}, formErrors: ["stop"] }, { status: 422 });
});

function mount() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/r/gadgets/create"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
}

describe("tabs", () => {
  it("switch by click and keyboard, keeping every panel mounted", async () => {
    mount();
    const schedule = await screen.findByRole("tab", { name: "Schedule" });
    const specsTab = screen.getByRole("tab", { name: "Specs" });
    expect(schedule).toHaveAttribute("aria-selected", "true");
    expect(screen.getByLabelText("Day")).toBeVisible();
    schedule.focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(specsTab).toHaveAttribute("aria-selected", "true");
    expect(specsTab).toHaveFocus();
    expect(screen.getByRole("group", { name: "Specs" })).toBeVisible();
    // The other panel stays in the document (its draft survives).
    expect(screen.getByLabelText("Day")).not.toBeVisible();
    await userEvent.keyboard("{Home}");
    expect(schedule).toHaveAttribute("aria-selected", "true");
  });

  it("open the tab of a server error and focus its input", async () => {
    answer = () => HttpResponse.json({ message: "x", code: "validation_failed", errors: { day: ["is invalid"] }, formErrors: [] }, { status: 422 });
    mount();
    await userEvent.type(await screen.findByLabelText(/^Name/), "Widget");
    await userEvent.click(screen.getByRole("tab", { name: "Specs" }));
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    const schedule = await screen.findByRole("tab", { name: /Schedule/ });
    await waitFor(() => expect(schedule).toHaveAttribute("aria-selected", "true"));
    expect(within(schedule).getByText("error", { exact: false })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText("Day")).toHaveFocus());
  });
});

describe("tabs and repeated errors", () => {
  it("re-open the errored tab on every failed save and focus its input", async () => {
    answer = () => HttpResponse.json({ message: "x", code: "validation_failed", errors: { day: ["is invalid"] }, formErrors: [] }, { status: 422 });
    mount();
    await userEvent.type(await screen.findByLabelText(/^Name/), "Widget");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => expect(screen.getByLabelText("Day")).toHaveFocus());
    // The user looks elsewhere, then saves again with the same error.
    await userEvent.click(screen.getByRole("tab", { name: /Specs/ }));
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => expect(screen.getByRole("tab", { name: /Schedule/ })).toHaveAttribute("aria-selected", "true"));
    await waitFor(() => expect(screen.getByLabelText("Day")).toHaveFocus());
  });
});

describe("key/value", () => {
  it("edits pairs and reports duplicates before sending", async () => {
    mount();
    await userEvent.type(await screen.findByLabelText(/^Name/), "Widget");
    await userEvent.click(screen.getByRole("tab", { name: "Specs" }));
    const add = screen.getByRole("button", { name: /Add entry/ });
    await userEvent.click(add);
    await userEvent.type(screen.getByLabelText("Key 1"), "size");
    await userEvent.type(screen.getByLabelText("Value 1"), "M");
    await userEvent.click(add);
    await userEvent.type(screen.getByLabelText("Key 2"), "size");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByText('has the key "size" twice')).toBeInTheDocument();
    expect(creates).toEqual([]);

    await userEvent.click(screen.getByRole("button", { name: "Remove entry 2" }));
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => expect(creates).toHaveLength(1));
    expect((creates[0] as Record<string, unknown>).specs).toEqual({ size: "M" });
  });

  it("keeps null, empty and reserved keys apart", async () => {
    expect(toFormValue(specs, null)).toBe("");
    expect(fromFormValue(specs, "")).toBeNull();
    expect(fromFormValue(specs, [])).toEqual({});
    const proto = fromFormValue(specs, toFormValue(specs, JSON.parse('{"__proto__": "x"}'))) as Record<string, string>;
    expect(Object.keys(proto)).toEqual(["__proto__"]);
    expect(JSON.stringify(proto)).toBe('{"__proto__":"x"}');
  });

  it("show legacy values as they are and can be cleared to not set", async () => {
    expect(toFormValue(specs, { a: 1 })).toBe(JSON.stringify({ a: 1 }, null, 2));
    expect(toFormValue(specs, null)).toBe("");
    mount();
    await userEvent.click(await screen.findByRole("tab", { name: "Specs" }));
    await userEvent.click(screen.getByRole("button", { name: /Add entry/ }));
    expect(screen.getByLabelText("Key 1")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Clear (not set)" }));
    expect(screen.getByText("Not set.")).toBeInTheDocument();
  });

  it("validates bounds in characters and leaves foreign values read-only", async () => {
    const check = async (value: unknown) => (await zodSchema([specs]).safeParseAsync({ specs: value })).success;
    expect(await check(toFormValue(specs, { a: "ü".repeat(40) }))).toBe(true);
    expect(await check(toFormValue(specs, { a: "ü".repeat(41) }))).toBe(false);
    expect(await check(toFormValue(specs, Object.fromEntries([1, 2, 3, 4, 5, 6].map((n) => [`k${n}`, "v"]))))).toBe(false);
    expect(await check(toFormValue(specs, { " ": "v" }))).toBe(false);
    const [protectedInput] = protectLossyJson([specs], { specs: { a: 1 } }) as InputSchema[];
    expect(protectedInput!.disabled).toBe(true);
  });
});
