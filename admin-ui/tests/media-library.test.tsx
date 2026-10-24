import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { ResourceSchema } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { GRID_PER_PAGE, GridThumb, MediaGrid } from "@/renderers/media-library";
import posts from "../../fixtures/contract/posts.schema.json";

const API = "http://localhost/admin/api";
const IMAGE = "0199d3a4-1b2c-7d3e-8f40-123456789abc";
const PDF = "0199d3a4-1b2c-7d3e-8f40-123456789abd";
let userId = "1";
let abilities: Record<string, boolean> = { viewAny: true, create: false, upload: true };

const schema = (): ResourceSchema => ({
  ...(posts as unknown as ResourceSchema),
  operation: "view",
  resource: { label: "Media", pluralLabel: "Media", slug: "media", titleField: "name" },
  abilities,
  table: {
    ...(posts as unknown as ResourceSchema).table,
    actions: [],
    filters: [],
    columns: [
      { name: "name", label: "Name", type: "text", valueType: "string", sortable: true, searchable: true },
      { name: "mime", label: "Mime", type: "text", valueType: "string", sortable: false, searchable: false },
    ],
  } as ResourceSchema["table"],
});
const record = (key: string, name: string, mime: string, alt?: string) => ({
  key, title: name, abilities: { view: true, update: true, delete: true }, data: { name, mime, ...(alt ? { alt } : {}) },
});
const uploads: { key: string | null; name: string }[] = [];
const listQueries: URLSearchParams[] = [];
let panelUploads: { maxBytes: number; maxConcurrent: number } | undefined;
let panelRequests = 0;
let trashed = false;

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: userId, name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId })),
  http.get("*/admin/api/panel", () => {
    panelRequests += 1;
    return HttpResponse.json({ brand: "T", user: { id: userId, name: "A", email: "a@b.c" }, navigation: [schema().resource], ...(panelUploads ? { uploads: panelUploads } : {}) });
  }),
  http.get("*/admin/api/resources/media/schema", () => HttpResponse.json(schema())),
  http.get("*/admin/api/resources/media/records/:key", () => HttpResponse.json({
    data: { ...record(IMAGE, "cat.png", "image/png", "A cat"), ...(trashed ? { trashed: true } : {}), data: { name: "cat.png", alt: "A cat", mime: "image/png", size: "2048", width: 4, height: 3 } },
  })),
  http.get("*/admin/api/resources/media/records", ({ request }) => {
    const params = new URL(request.url).searchParams;
    listQueries.push(params);
    return HttpResponse.json({
      data: [record(IMAGE, "cat.png", "image/png", "A cat"), record(PDF, "doc.pdf", "application/pdf")],
      meta: { page: 1, perPage: Number(params.get("per_page") ?? 25), total: 2, lastPage: 1 },
    });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  uploads.length = 0;
  listQueries.length = 0;
  panelUploads = undefined;
  panelRequests = 0;
  trashed = false;
  userId = "1";
  abilities = { viewAny: true, create: false, upload: true };
  window.localStorage.clear();
});

async function mount(path = "/r/media", ready = "cat.png") {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: API }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: [path] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  await screen.findAllByText(ready);
}

it("shows files as thumbnails of their authorized URL, or as a table, remembered per user", async () => {
  await mount();
  const image = screen.getByRole("img", { name: "A cat" });
  expect(image).toHaveAttribute("src", `${API}/media/${IMAGE}/file`);
  expect(image).toHaveAttribute("loading", "lazy");
  // Only images are loaded: other files show an icon.
  expect(screen.getAllByRole("img")).toHaveLength(1);
  expect(screen.queryByRole("table")).toBeNull();

  await userEvent.click(screen.getByRole("button", { name: "List" }));
  expect(await screen.findByRole("table")).toBeInTheDocument();
  expect(screen.queryByRole("img", { name: "A cat" })).toBeNull();

  // Remembered for this user…
  cleanup();
  await mount();
  expect(await screen.findByRole("table")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "List" })).toHaveAttribute("aria-pressed", "true");
  // …not for another one.
  cleanup();
  userId = "2";
  await mount();
  await waitFor(() => expect(screen.queryByRole("table")).toBeNull());
  expect(screen.getByRole("img", { name: "A cat" })).toBeInTheDocument();
});

it("uploads through the shared queue: a refused file is not retried, a failed one is, with its key", async () => {
  // b.png is refused (422: sending it again changes nothing), c.png fails once (503: worth a retry).
  const failures: Record<string, number[]> = { "b.png": [422], "c.png": [503] };
  // Node's fetch cannot send jsdom's FormData: answer at the fetch boundary.
  const real = globalThis.fetch;
  let inFlight = 0;
  vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
    if (!url.endsWith("/media")) return real(url, init);
    // The panel gives no upload limits here: one at a time.
    expect(inFlight).toBe(0);
    inFlight += 1;
    await new Promise((resolve) => setTimeout(resolve, 5));
    inFlight -= 1;
    const file = (init.body as FormData).get("file") as File;
    uploads.push({ key: (init.headers as Record<string, string>)["Idempotency-Key"] ?? null, name: file.name });
    const status = failures[file.name]?.shift();
    if (status === 422) {
      const body = { code: "validation_failed", message: "Invalid", errors: { file: ["is not an accepted file type"] } };
      return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
    }
    if (status === 503) return new Response(JSON.stringify({ code: "unavailable", message: "Down" }), { status, headers: { "Content-Type": "application/json" } });
    const data = { id: crypto.randomUUID(), name: file.name, mime: "image/png", size: 1, inline: true };
    return new Response(JSON.stringify({ data }), { status: 201, headers: { "Content-Type": "application/json" } });
  });
  onTestFinished(() => {
    vi.unstubAllGlobals();
  });
  await mount();
  const listed = listQueries.length;
  const files = [new File(["a"], "a.png", { type: "image/png" }), new File(["b"], "b.png", { type: "image/png" }), new File(["c"], "c.png", { type: "image/png" })];
  await userEvent.upload(screen.getByTestId("media-upload-input"), files);
  const queue = await screen.findByRole("region", { name: "Uploads" });
  await waitFor(() => expect(within(queue).getByText("Uploads finished")).toBeInTheDocument());
  expect(uploads.map((u) => u.name)).toEqual(["a.png", "b.png", "c.png"]);
  expect(within(queue).getByText("1 file uploaded")).toBeInTheDocument();
  // The list is fetched again once a file is in.
  await waitFor(() => expect(listQueries.length).toBeGreaterThan(listed));
  const alerts = within(queue).getAllByRole("alert");
  expect(alerts[0]).toHaveTextContent("is not an accepted file type");
  expect(within(queue).queryByRole("button", { name: "Retry uploading b.png" })).toBeNull();

  await userEvent.click(within(queue).getByRole("button", { name: "Retry uploading c.png" }));
  await waitFor(() => expect(within(queue).getByText("2 files uploaded")).toBeInTheDocument());
  expect(uploads[3]).toEqual({ key: uploads[2]!.key, name: "c.png" });
  expect(new Set(uploads.map((u) => u.key)).size).toBe(3);
  await userEvent.click(within(queue).getByRole("button", { name: "Clear the finished uploads" }));
  expect(screen.queryByRole("region", { name: "Uploads" })).toBeNull();
});

it("checks the server's size limit before sending", async () => {
  panelUploads = { maxBytes: 2, maxConcurrent: 2 };
  const sent: string[] = [];
  const real = globalThis.fetch;
  vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
    if (!url.endsWith("/media")) return real(url, init);
    sent.push(((init.body as FormData).get("file") as File).name);
    const data = { id: crypto.randomUUID(), name: "ok.png", mime: "image/png", size: 1, inline: true };
    return new Response(JSON.stringify({ data }), { status: 201, headers: { "Content-Type": "application/json" } });
  });
  onTestFinished(() => {
    vi.unstubAllGlobals();
  });
  await mount();
  await waitFor(() => expect(panelRequests).toBeGreaterThan(0));
  await userEvent.upload(screen.getByTestId("media-upload-input"), [new File(["123"], "big.png", { type: "image/png" }), new File(["1"], "ok.png", { type: "image/png" })]);
  const queue = await screen.findByRole("region", { name: "Uploads" });
  await waitFor(() => expect(within(queue).getByText("1 file uploaded")).toBeInTheDocument());
  expect(sent).toEqual(["ok.png"]);
  expect(within(queue).getByRole("alert")).toHaveTextContent("big.png");
  expect(within(queue).queryByRole("button", { name: "Retry uploading big.png" })).toBeNull();
});

it("bounds the grid's page to 24 thumbnails, the table keeps the asked size", async () => {
  await mount("/r/media?per_page=100");
  await waitFor(() => expect(listQueries.at(-1)!.get("per_page")).toBe("24"));
  const sizes = screen.getByRole("combobox", { name: "Per page" });
  expect([...sizes.querySelectorAll("option")].map((o) => o.value)).toEqual(["10", "24"]);
  await userEvent.click(screen.getByRole("button", { name: "List" }));
  await waitFor(() => expect(listQueries.at(-1)!.get("per_page")).toBe("100"));
  // A smaller page is kept in the grid.
  cleanup();
  window.localStorage.clear();
  await mount("/r/media?per_page=10");
  await waitFor(() => expect(listQueries.at(-1)!.get("per_page")).toBe("10"));
});

it("never shows more than a grid page, whatever rows it is given (a table's page meanwhile)", () => {
  const session = createSession({ basePath: "", apiPath: API }, new QueryClient(), () => {});
  const many = Array.from({ length: 100 }, (_, i) => record(`0192f7a0-0000-7000-8000-${String(i).padStart(12, "0")}`, `f${i}.png`, "image/png", `F${i}`));
  render(<SessionContext.Provider value={session}><MediaGrid rows={many} actions={() => null} badge={() => null} /></SessionContext.Provider>);
  expect(screen.getAllByRole("listitem")).toHaveLength(GRID_PER_PAGE);
});

it("tries a thumbnail that failed again after the next fetch, even unchanged", async () => {
  const session = createSession({ basePath: "", apiPath: API }, new QueryClient(), () => {});
  const cat = record(IMAGE, "cat.png", "image/png", "A cat");
  const view = render(<SessionContext.Provider value={session}><GridThumb record={cat} generation={1} /></SessionContext.Provider>);
  fireEvent.error(screen.getByRole("img", { name: "A cat" }));
  expect(screen.queryByRole("img", { name: "A cat" })).toBeNull();
  // The same fetch: still the icon.
  view.rerender(<SessionContext.Provider value={session}><GridThumb record={cat} generation={1} /></SessionContext.Provider>);
  expect(screen.queryByRole("img", { name: "A cat" })).toBeNull();
  // Refetched (the same, structurally shared record): loaded again.
  view.rerender(<SessionContext.Provider value={session}><GridThumb record={cat} generation={2} /></SessionContext.Provider>);
  expect(screen.getByRole("img", { name: "A cat" })).toHaveAttribute("src", `${API}/media/${IMAGE}/file`);
});

it("offers uploads only to users who may upload", async () => {
  abilities = { viewAny: true, create: false, upload: false };
  await mount();
  expect(screen.queryByRole("button", { name: "Upload files" })).toBeNull();
  expect(screen.getByRole("button", { name: "Grid" })).toBeInTheDocument();
});

it("shows the file, its alternative text and what was detected on its record page", async () => {
  await mount(`/r/media/records/${IMAGE}`);
  const image = await screen.findByRole("img", { name: "A cat" });
  expect(image).toHaveAttribute("src", `${API}/media/${IMAGE}/file`);
  expect(screen.getByText("image/png · 2,048 bytes · 4 × 3")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: /Edit/ })).toBeInTheDocument();
});

it("shows a trashed file as unavailable, without a download link", async () => {
  trashed = true;
  await mount(`/r/media/records/${IMAGE}`, "image/png · 2,048 bytes · 4 × 3");
  expect(screen.getByRole("img", { name: "File unavailable" })).toBeInTheDocument();
  expect(screen.queryByRole("img", { name: "A cat" })).toBeNull();
  expect(screen.queryByRole("link", { name: /cat\.png/ })).toBeNull();
});
