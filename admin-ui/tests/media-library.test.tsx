import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { ResourceSchema } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
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
let failNext: number | null = null;

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: userId, name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "T", user: { id: userId, name: "A", email: "a@b.c" }, navigation: [schema().resource] })),
  http.get("*/admin/api/resources/media/schema", () => HttpResponse.json(schema())),
  http.get("*/admin/api/resources/media/records", () => HttpResponse.json({
    data: [record(IMAGE, "cat.png", "image/png", "A cat"), record(PDF, "doc.pdf", "application/pdf")],
    meta: { page: 1, perPage: 25, total: 2, lastPage: 1 },
  })),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  uploads.length = 0;
  failNext = null;
  userId = "1";
  abilities = { viewAny: true, create: false, upload: true };
  window.localStorage.clear();
});

async function mount() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: API }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/r/media"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  await screen.findByText("cat.png");
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

it("uploads several files one at a time, and retries a failed one with its key", async () => {
  failNext = 2;
  // Node's fetch cannot send jsdom's FormData: answer at the fetch boundary.
  const real = globalThis.fetch;
  let inFlight = 0;
  vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
    if (!url.endsWith("/media")) return real(url, init);
    expect(inFlight).toBe(0);
    inFlight += 1;
    await new Promise((resolve) => setTimeout(resolve, 5));
    inFlight -= 1;
    const file = (init.body as FormData).get("file") as File;
    uploads.push({ key: (init.headers as Record<string, string>)["Idempotency-Key"] ?? null, name: file.name });
    if (failNext === uploads.length) {
      failNext = null;
      const body = { code: "validation_failed", message: "Invalid", errors: { file: ["is not an accepted file type"] } };
      return new Response(JSON.stringify(body), { status: 422, headers: { "Content-Type": "application/json" } });
    }
    const data = { id: crypto.randomUUID(), name: file.name, mime: "image/png", size: 1, inline: true };
    return new Response(JSON.stringify({ data }), { status: 201, headers: { "Content-Type": "application/json" } });
  });
  onTestFinished(() => vi.unstubAllGlobals());
  await mount();
  const files = [new File(["a"], "a.png", { type: "image/png" }), new File(["b"], "b.png", { type: "image/png" }), new File(["c"], "c.png", { type: "image/png" })];
  await userEvent.upload(screen.getByTestId("media-upload-input"), files);
  const queue = await screen.findByRole("region", { name: "Uploads" });
  await waitFor(() => expect(within(queue).getByText("Uploads finished")).toBeInTheDocument());
  expect(uploads.map((u) => u.name)).toEqual(["a.png", "b.png", "c.png"]);
  expect(within(queue).getAllByText("Uploaded")).toHaveLength(2);
  expect(within(queue).getByText("Failed")).toBeInTheDocument();
  expect(within(queue).getByRole("alert")).toHaveTextContent("is not an accepted file type");

  await userEvent.click(within(queue).getByRole("button", { name: "Retry b.png" }));
  await waitFor(() => expect(within(queue).getAllByText("Uploaded")).toHaveLength(3));
  expect(uploads[3]).toEqual({ key: uploads[1]!.key, name: "b.png" });
  expect(new Set(uploads.map((u) => u.key)).size).toBe(3);
  await userEvent.click(within(queue).getByRole("button", { name: "Clear the finished uploads" }));
  expect(screen.queryByRole("region", { name: "Uploads" })).toBeNull();
});

it("offers uploads only to users who may upload", async () => {
  abilities = { viewAny: true, create: false, upload: false };
  await mount();
  expect(screen.queryByRole("button", { name: "Upload files" })).toBeNull();
  expect(screen.getByRole("button", { name: "Grid" })).toBeInTheDocument();
});
