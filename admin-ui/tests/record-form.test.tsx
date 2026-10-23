import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { ResourceSchema } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { stayOnPurpose } from "@/lib/leave";
import { defaultValues, inputsOf } from "@/schema/form";
import { editResolver } from "@/schema/resolver";
import create from "../../fixtures/contract/kitchen-sink.schema.create.json";
import edit from "../../fixtures/contract/kitchen-sink.schema.edit.json";
import records from "../../fixtures/contract/kitchen-sink.records.json";

const editSchema = edit as unknown as ResourceSchema;
const createSchema = create as unknown as ResourceSchema;
const record = records.data[0]!;
let patches: unknown[] = [];
let recordFetches = 0;
let viewable = true;
let recordFails = false;
let agreedWriteOnly = false;
let csrfUser = "1";
let recordOwner = "1";
let creates: unknown[] = [];

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1", name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: csrfUser })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({ brand: "Test", user: { id: "1", name: "Ada", email: "a@b.c" }, navigation: [editSchema.resource] }),
  ),
  http.get("*/admin/api/resources/gadgets/schema", ({ request }) => {
    const create = new URL(request.url).searchParams.get("operation") === "create";
    const schema = structuredClone(create ? createSchema : editSchema);
    if (agreedWriteOnly) {
      const agreed = inputsOf(schema.form.components).find((i) => i.name === "agreed")!;
      agreed.writeOnly = true;
    }
    return HttpResponse.json({ ...schema, abilities: { ...schema.abilities, view: viewable } });
  }),
  http.get("*/admin/api/resources/gadgets/records/1", () => {
    recordFetches += 1;
    if (recordFails) return HttpResponse.json({ message: "Server error.", code: "internal_error" }, { status: 500 });
    return HttpResponse.json({ data: record }, { headers: { "X-Rustcms-User": recordOwner } });
  }),
  http.get("*/admin/api/resources/:slug/fields/:field/options", () => HttpResponse.json({ data: [] })),
  http.post("*/admin/api/resources/gadgets/records", async ({ request }) => {
    creates.push(await request.json());
    return HttpResponse.json({ message: "x", code: "validation_failed", errors: {}, formErrors: ["stop"] }, { status: 422 });
  }),
  http.patch("*/admin/api/resources/gadgets/records/1", async ({ request }) => {
    patches.push(await request.json());
    return HttpResponse.json(
      { message: "The given data was invalid.", code: "validation_failed", errors: { name: ["has already been taken"] }, formErrors: [] },
      { status: 422 },
    );
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  // A test that switched users "left" the page on purpose (jsdom does not navigate).
  stayOnPurpose();
  patches = [];
  recordFetches = 0;
  viewable = true;
  recordFails = false;
  agreedWriteOnly = false;
  csrfUser = "1";
  recordOwner = "1";
  creates = [];
});

let router: ReturnType<typeof createAppRouter>;

function mount(path: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: [path] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  return { queryClient, session };
}

describe("edit form", () => {
  it("keeps the draft across refetches and a rejected save", async () => {
    const { queryClient } = mount("/r/gadgets/records/1/edit");
    const name = await screen.findByLabelText(/^Name/);
    await userEvent.clear(name);
    await userEvent.type(name, "Draft name");

    // A focus refetch brings the same record back.
    await act(() => queryClient.invalidateQueries());
    await waitFor(() => expect(recordFetches).toBeGreaterThan(1));
    expect(screen.getByLabelText(/^Name/)).toHaveValue("Draft name");

    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    expect(await screen.findByText("has already been taken")).toBeInTheDocument();
    expect(patches).toEqual([{ name: "Draft name" }]);
    expect(screen.getByLabelText(/^Name/)).toHaveValue("Draft name");
  });

  it("asks before leaving with unsaved changes", async () => {
    mount("/r/gadgets/records/1/edit");
    // Untouched: leaving needs no confirmation.
    await screen.findByLabelText(/^Name/);
    await act(() => router.navigate({ to: "/r/$slug", params: { slug: "gadgets" } }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/r/gadgets"));

    await act(() => router.navigate({ to: "/r/$slug/records/$key/edit", params: { slug: "gadgets", key: "1" } }));
    const name = await screen.findByLabelText(/^Name/);
    await userEvent.clear(name);
    await userEvent.type(name, "Unsaved");
    await userEvent.click(screen.getByRole("link", { name: "Cancel" }));
    const dialog = await screen.findByRole("dialog", { name: "Discard unsaved changes?" });
    // Keep editing: still here, with the draft.
    await userEvent.click(within(dialog).getByRole("button", { name: "Keep editing" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Discard unsaved changes?" })).toBeNull());
    expect(router.state.location.pathname).toBe("/r/gadgets/records/1/edit");
    expect(screen.getByLabelText(/^Name/)).toHaveValue("Unsaved");
    // A record whose key is `login` is not the sign-in page: still asked.
    void router.navigate({ to: "/r/$slug/records/$key", params: { slug: "gadgets", key: "login" } });
    const asked = await screen.findByRole("dialog", { name: "Discard unsaved changes?" });
    await userEvent.click(within(asked).getByRole("button", { name: "Keep editing" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Discard unsaved changes?" })).toBeNull());
    expect(router.state.location.pathname).toBe("/r/gadgets/records/1/edit");
    // Discard: leaves.
    await userEvent.click(screen.getByRole("link", { name: "Cancel" }));
    const again = await screen.findByRole("dialog", { name: "Discard unsaved changes?" });
    await userEvent.click(within(again).getByRole("button", { name: "Discard changes" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/r/gadgets"));
  });

  it("leaves without asking when an edit is put back as it was", async () => {
    mount("/r/gadgets/records/1/edit");
    const name = await screen.findByLabelText(/^Name/);
    const original = (name as HTMLInputElement).value;
    await userEvent.type(name, "x");
    await userEvent.type(name, "{Backspace}");
    expect(name).toHaveValue(original);
    await userEvent.click(screen.getByRole("link", { name: "Cancel" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/r/gadgets"));
    expect(screen.queryByRole("dialog", { name: "Discard unsaved changes?" })).toBeNull();
  });

  it("leaves without asking after a save", async () => {
    server.use(
      http.patch("*/admin/api/resources/gadgets/records/1", () => HttpResponse.json({ data: record })),
    );
    try {
      mount("/r/gadgets/records/1/edit");
      const name = await screen.findByLabelText(/^Name/);
      await userEvent.clear(name);
      await userEvent.type(name, "Saved");
      await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
      await waitFor(() => expect(router.state.location.pathname).toBe("/r/gadgets/records/1"));
      expect(screen.queryByRole("dialog", { name: "Discard unsaved changes?" })).toBeNull();
    } finally {
      server.resetHandlers();
    }
  });

  it("keeps the draft when a background refresh fails and recovers", async () => {
    const { queryClient } = mount("/r/gadgets/records/1/edit");
    const name = await screen.findByLabelText(/^Name/);
    await userEvent.clear(name);
    await userEvent.type(name, "Unsaved draft");
    recordFails = true;
    await act(() => queryClient.invalidateQueries());
    expect(await screen.findByText(/Could not refresh this page/)).toBeInTheDocument();
    expect(screen.getByLabelText(/^Name/)).toHaveValue("Unsaved draft");
    recordFails = false;
    await act(() => queryClient.invalidateQueries());
    await waitFor(() => expect(screen.queryByText(/Could not refresh this page/)).toBeNull());
    expect(screen.getByLabelText(/^Name/)).toHaveValue("Unsaved draft");
  });

  it("saves a single field when the record may be updated but not viewed", async () => {
    viewable = false;
    mount("/r/gadgets/records/1/edit");
    expect(await screen.findByText(/may update this record but not view it/)).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Notes"), "only this");
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(patches).toEqual([{ notes: "only this" }]));
    expect(recordFetches).toBe(0);
  });

  it("keeps a blind draft when a refetch grants view", async () => {
    viewable = false;
    const { queryClient } = mount("/r/gadgets/records/1/edit");
    await userEvent.type(await screen.findByLabelText("Notes"), "draft");
    viewable = true;
    await act(() => queryClient.invalidateQueries());
    await waitFor(() => expect(screen.getByLabelText("Notes")).toHaveValue("draft"));
    expect(screen.getByText(/may update this record but not view it/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(patches).toEqual([{ notes: "draft" }]));
    expect(recordFetches).toBe(0);
  });

  it("never shows data produced for another account", async () => {
    const assign = vi.fn();
    vi.stubGlobal("location", { ...window.location, assign });
    try {
      // The page was built for user 1 (`/auth/me`); the cookie now belongs to user 2.
      server.use(http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: "1", name: "A", email: "a@b.c" }, navigation: [] }, { headers: { "X-Rustcms-User": "1" } })));
      recordOwner = "2";
      mount("/r/gadgets/records/1/edit");
      await waitFor(() => expect(assign).toHaveBeenCalled());
      expect(screen.queryByDisplayValue("Widget")).toBeNull();
    } finally {
      vi.unstubAllGlobals();
      server.resetHandlers();
    }
  });

  it("retries an unknown create safely and continues to the record", async () => {
    const sent: { key: string | null; body: unknown }[] = [];
    let status = 504;
    server.use(
      http.post("*/admin/api/resources/gadgets/records", async ({ request }) => {
        sent.push({ key: request.headers.get("Idempotency-Key"), body: await request.json() });
        return status === 504 ? new HttpResponse(null, { status: 504 }) : HttpResponse.json({ data: record }, { status: 201 });
      }),
    );
    try {
      mount("/r/gadgets/create");
      await userEvent.type(await screen.findByLabelText(/^Name/), "Once");
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      expect(await screen.findByRole("button", { name: "Retry safely" })).toBeInTheDocument();
      // The form is locked: the retry sends what was submitted, not later edits.
      expect(screen.getByLabelText(/^Name/)).toBeDisabled();
      status = 201;
      await userEvent.click(screen.getByRole("button", { name: "Retry safely" }));
      await waitFor(() => expect(router.state.location.pathname).toBe("/r/gadgets/records/1"));
      expect(sent).toHaveLength(2);
      expect(sent[1]).toEqual(sent[0]);
    } finally {
      server.resetHandlers();
    }
  });

  it("a retried create from another form leaves this form and its draft alone", async () => {
    let status = 504;
    server.use(
      http.post("*/admin/api/resources/gadgets/records", () =>
        status === 504 ? new HttpResponse(null, { status: 504 }) : HttpResponse.json({ data: record }, { status: 201 }),
      ),
    );
    try {
      mount("/r/gadgets/create");
      await userEvent.type(await screen.findByLabelText(/^Name/), "First");
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      await screen.findByRole("button", { name: "Retry safely" });
      // A new create form (another draft) shows the same pause.
      await act(() => router.navigate({ to: "/" }));
      await act(() => router.navigate({ to: "/r/$slug/create", params: { slug: "gadgets" } }));
      status = 201;
      await userEvent.click(await screen.findByRole("button", { name: "Retry safely" }));
      expect(await screen.findByText(/The change was applied/)).toBeInTheDocument();
      expect(router.state.location.pathname).toBe("/r/gadgets/create");
    } finally {
      server.resetHandlers();
    }
  });

  it("keeps writes paused for a resource across navigation while one is pending", async () => {
    let release: () => void = () => {};
    let posts = 0;
    server.use(
      http.post("*/admin/api/resources/gadgets/records", async () => {
        posts += 1;
        await new Promise<void>((r) => (release = r));
        return new HttpResponse(null, { status: 504 });
      }),
    );
    try {
      mount("/r/gadgets/create");
      await userEvent.type(await screen.findByLabelText(/^Name/), "Once");
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      await waitFor(() => expect(posts).toBe(1));
      // Leave and reopen the form while the first request is still pending.
      await act(() => router.navigate({ to: "/" }));
      await act(() => router.navigate({ to: "/r/$slug/create", params: { slug: "gadgets" } }));
      await userEvent.type(await screen.findByLabelText(/^Name/), "Once");
      release();
      await waitFor(() => expect(screen.getByRole("button", { name: "Create" })).toBeDisabled());
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      expect(posts).toBe(1);
    } finally {
      server.resetHandlers();
    }
  });

  it("never sends a queued write after the previous one turned out uncertain", async () => {
    let release: () => void = () => {};
    let posts = 0;
    server.use(
      http.post("*/admin/api/resources/gadgets/records", async () => {
        posts += 1;
        await new Promise<void>((r) => (release = r));
        return new HttpResponse(null, { status: 504 });
      }),
    );
    try {
      mount("/r/gadgets/create");
      await userEvent.type(await screen.findByLabelText(/^Name/), "First");
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      await waitFor(() => expect(posts).toBe(1));
      await act(() => router.navigate({ to: "/" }));
      await act(() => router.navigate({ to: "/r/$slug/create", params: { slug: "gadgets" } }));
      await userEvent.type(await screen.findByLabelText(/^Name/), "Second");
      // Submitted while the first is still pending: it waits, then is refused.
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      release();
      expect(await screen.findByText(/may or may not have been applied/)).toBeInTheDocument();
      expect(posts).toBe(1);
    } finally {
      server.resetHandlers();
    }
  });

  it("drops queued writes when the session changes before they run", async () => {
    let release: () => void = () => {};
    let posts = 0;
    server.use(
      http.post("*/admin/api/resources/gadgets/records", async () => {
        posts += 1;
        await new Promise<void>((r) => (release = r));
        return HttpResponse.json({ message: "CSRF token mismatch.", code: "csrf_mismatch" }, { status: 403 });
      }),
    );
    try {
      const { session } = mount("/r/gadgets/create");
      await userEvent.type(await screen.findByLabelText(/^Name/), "A's first");
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      await waitFor(() => expect(posts).toBe(1));
      await act(() => router.navigate({ to: "/" }));
      await act(() => router.navigate({ to: "/r/$slug/create", params: { slug: "gadgets" } }));
      await userEvent.type(await screen.findByLabelText(/^Name/), "A's second");
      await userEvent.click(screen.getByRole("button", { name: "Create" })); // queued behind the first
      act(() => session.signedOut({ broadcast: false })); // A signs out (B may sign in next)
      release();
      await new Promise((r) => setTimeout(r, 100));
      expect(posts).toBe(1);
    } finally {
      server.resetHandlers();
    }
  });

  it("a save finishing after leaving the form does not take over another page", async () => {
    let release: () => void = () => {};
    server.use(
      http.post("*/admin/api/resources/gadgets/records", async () => {
        await new Promise<void>((r) => (release = r));
        return HttpResponse.json({ data: { ...record, key: "9" } }, { status: 201 });
      }),
    );
    try {
      mount("/r/gadgets/create");
      await userEvent.type(await screen.findByLabelText(/^Name/), "Slow one");
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      await act(() => router.navigate({ to: "/r/$slug/records/$key/edit", params: { slug: "gadgets", key: "1" } }));
      const name = await screen.findByLabelText(/^Name/);
      await userEvent.clear(name);
      await userEvent.type(name, "Draft B");
      release();
      await new Promise((r) => setTimeout(r, 100));
      expect(router.state.location.pathname).toBe("/r/gadgets/records/1/edit");
      expect(screen.getByLabelText(/^Name/)).toHaveValue("Draft B");
    } finally {
      server.resetHandlers();
    }
  });

  it("never writes a draft as another account that took over the cookie", async () => {
    const assign = vi.fn();
    vi.stubGlobal("location", { ...window.location, assign });
    try {
      mount("/r/gadgets/records/1/edit");
      const name = await screen.findByLabelText(/^Name/);
      await userEvent.type(name, " by A");
      csrfUser = "2"; // B signed in elsewhere in this browser
      await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
      expect(await screen.findByText(/signed in as someone else/)).toBeInTheDocument();
      expect(patches).toEqual([]);
      expect(assign).toHaveBeenCalled();
      // The reload stalls and the user retries: still nothing is written.
      await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
      await new Promise((r) => setTimeout(r, 100));
      expect(patches).toEqual([]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("sends the displayed off value of a non-nullable, non-required toggle on create", async () => {
    server.use(
      http.get("*/admin/api/resources/gadgets/schema", () => {
        const schema = structuredClone(createSchema);
        const agreed = inputsOf(schema.form.components).find((i) => i.name === "agreed")!;
        agreed.rules = [];
        return HttpResponse.json(schema);
      }),
    );
    try {
      mount("/r/gadgets/create");
      await userEvent.type(await screen.findByLabelText(/^Name/), "Plain");
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      await waitFor(() => expect(creates).toEqual([{ name: "Plain", active: true, agreed: false }]));
    } finally {
      server.resetHandlers();
    }
  });

  it("sends untouched defaults exactly as declared", async () => {
    server.use(
      http.get("*/admin/api/resources/gadgets/schema", () => {
        const schema = structuredClone(createSchema);
        const all = inputsOf(schema.form.components);
        Object.assign(all.find((i) => i.name === "notes")!, { default: "" });
        Object.assign(all.find((i) => i.name === "happened_at")!, { default: "2026-10-12T09:30:00.123Z" });
        return HttpResponse.json(schema);
      }),
    );
    try {
      mount("/r/gadgets/create");
      await userEvent.type(await screen.findByLabelText(/^Name/), "Exact");
      await userEvent.click(screen.getByRole("button", { name: "Create" }));
      await waitFor(() => expect(creates).toHaveLength(1));
      expect(creates[0]).toMatchObject({ notes: "", happened_at: "2026-10-12T09:30:00.123Z" });
    } finally {
      server.resetHandlers();
    }
  });

  it("locks the form while a save is in flight", async () => {
    let release: () => void = () => {};
    server.use(
      http.patch("*/admin/api/resources/gadgets/records/1", async () => {
        await new Promise<void>((r) => (release = r));
        return HttpResponse.json({ message: "x", code: "validation_failed", errors: {}, formErrors: ["no"] }, { status: 422 });
      }),
    );
    try {
      mount("/r/gadgets/records/1/edit");
      const name = await screen.findByLabelText(/^Name/);
      await userEvent.type(name, " v2");
      await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
      await waitFor(() => expect(screen.getByLabelText(/^Name/)).toBeDisabled());
      release();
      await waitFor(() => expect(screen.getByLabelText(/^Name/)).not.toBeDisabled());
    } finally {
      server.resetHandlers();
    }
  });

  it("blocks resubmitting a create whose outcome is unknown", async () => {
    server.use(http.post("*/admin/api/resources/gadgets/records", () => new HttpResponse(null, { status: 504 })));
    mount("/r/gadgets/create");
    await userEvent.type(await screen.findByLabelText(/^Name/), "Maybe saved");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByText(/may have been applied/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    server.resetHandlers();
  });

  it("creates with only the fields the user filled in", async () => {
    mount("/r/gadgets/create");
    await userEvent.type(await screen.findByLabelText(/^Name/), "New gadget");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    // `active` shows and sends its declared default (on); the required `agreed`
    // checkbox without a default is sent as the "off" it shows; the rest is omitted.
    expect(screen.getByRole("switch", { name: "Active" })).toHaveAttribute("aria-checked", "true");
    await waitFor(() => expect(creates).toEqual([{ name: "New gadget", active: true, agreed: false }]));
  });

  it("submits a deliberate false on a write-only field of a viewable record", async () => {
    agreedWriteOnly = true;
    mount("/r/gadgets/records/1/edit");
    const agreed = await screen.findByLabelText("Terms agreed");
    await userEvent.click(agreed);
    await userEvent.click(agreed);
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(patches).toEqual([{ agreed: false }]));
  });

  it("submits deliberate false and cleared values on a blind edit", async () => {
    viewable = false;
    mount("/r/gadgets/records/1/edit");
    const active = await screen.findByRole("switch", { name: "Active" });
    await userEvent.click(active);
    await userEvent.click(active);
    await userEvent.type(screen.getByLabelText("Notes"), "x");
    await userEvent.clear(screen.getByLabelText("Notes"));
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(patches).toEqual([{ active: false, notes: null }]));
  });
});

describe("editResolver", () => {
  const inputs = inputsOf(editSchema.form.components);
  const blank = defaultValues(inputs);
  const options = { fields: {}, shouldUseNativeValidation: false } as never;

  it("checks only changed fields on edit", async () => {
    const untouched = await editResolver(inputs, true, () => false)(blank, undefined, options);
    expect(untouched.errors).toEqual({});
    const cleared = await editResolver(inputs, true, (n) => n === "name")(blank, undefined, options);
    expect(Object.keys(cleared.errors)).toEqual(["name"]);
  });

  it("checks everything on create", async () => {
    const result = await editResolver(inputs, false, () => false)(blank, undefined, options);
    expect(Object.keys(result.errors)).toContain("name");
  });
});
