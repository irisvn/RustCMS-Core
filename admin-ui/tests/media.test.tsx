import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { useState } from "react";
import { SessionContext } from "@/api/context";
import type { InputSchema } from "@/api/types";
import { createSession } from "@/app/session";
import { asDocument, isEmptyDocument, mediaIdOf, safeHref, toDocument, toEditor, type DocNode } from "@/lib/rich-text";
import { FileUploadWidget, MediaThumb } from "@/renderers/media";
import { RichEditorWidget, RichTextView } from "@/renderers/rich-text";

const API = "http://localhost/admin/api";
const ID = "0199d3a4-1b2c-7d3e-8f40-123456789abc";
const keys: (string | null)[] = [];

const server = setupServer(
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  keys.length = 0;
});

function Providers({ children }: { children: React.ReactNode }) {
  const client = new QueryClient();
  const session = createSession({ basePath: "/admin", apiPath: API }, client, () => {});
  return (
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    </SessionContext.Provider>
  );
}

describe("documents", () => {
  it("keep only what the server accepts, with images as media ids", () => {
    const editor: DocNode = {
      type: "doc",
      content: [
        { type: "heading", attrs: { level: 1 }, content: [{ type: "text", text: "T" }] },
        { type: "paragraph", content: [
          { type: "text", text: "x", marks: [{ type: "bold" }, { type: "underline" }, { type: "link", attrs: { href: "javascript:alert(1)" } }] },
          { type: "text", text: "y", marks: [{ type: "link", attrs: { href: "https://ok.test", target: "_blank" } }] },
        ] },
        { type: "image", attrs: { src: `${API}/media/${ID}/file`, alt: "a" } },
        { type: "image", attrs: { src: "https://evil.test/x.png" } },
        { type: "orderedList", attrs: { start: 3 }, content: [{ type: "listItem", content: [{ type: "paragraph" }] }] },
        { type: "table" },
      ],
    };
    expect(toDocument(editor, API)).toEqual({
      type: "doc",
      version: 1,
      content: [
        { type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "T" }] },
        { type: "paragraph", content: [
          { type: "text", text: "x", marks: [{ type: "bold" }] },
          { type: "text", text: "y", marks: [{ type: "link", attrs: { href: "https://ok.test" } }] },
        ] },
        { type: "image", attrs: { media: ID, alt: "a" } },
        { type: "orderedList", content: [{ type: "listItem", content: [{ type: "paragraph", content: [] }] }] },
      ],
    });
    const doc = asDocument({ type: "doc", version: 1, content: [{ type: "image", attrs: { media: ID, alt: "" } }] })!;
    expect(toEditor(doc, API).content?.[0]?.attrs?.src).toBe(`${API}/media/${ID}/file`);
    expect(mediaIdOf(API, `${API}/media/../x/file`)).toBeNull();
    expect(isEmptyDocument({ type: "doc", version: 1, content: [{ type: "paragraph" }] })).toBe(true);
    expect(safeHref("mailto:a@b.c") && !safeHref("javascript:x") && !safeHref("https://a b")).toBe(true);
  });

  it("are edited only when the editor can hold all of them", () => {
    const ok = { type: "doc", version: 1, content: [{ type: "bulletList", content: [{ type: "listItem", content: [{ type: "paragraph", content: [{ type: "text", text: "x" }] }] }] }] };
    expect(asDocument(ok)).not.toBeNull();
    for (const content of [
      [{ type: "table", content: [] }],
      [{ type: "listItem", content: [] }],
      [{ type: "paragraph", content: [{ type: "image", attrs: { media: ID } }] }],
      [{ type: "paragraph", content: [{ type: "text", text: "x", marks: [{ type: "underline" }] }] }],
      [{ type: "bulletList", content: [{ type: "paragraph" }] }],
      [{ type: "image", attrs: { media: "not-a-uuid" } }],
    ]) {
      expect(asDocument({ type: "doc", version: 1, content }), JSON.stringify(content)).toBeNull();
    }
    // Alt text is cut by code points, like the server.
    const alt = "😀".repeat(600);
    const doc = toDocument({ type: "doc", content: [{ type: "image", attrs: { src: `${API}/media/${ID}/file`, alt } }] }, API);
    expect([...String(doc.content[0]!.attrs!.alt)]).toHaveLength(500);
    expect(safeHref(`https://x.test/${"é".repeat(1100)}`)).toBe(false);
    // Every control the server refuses (Rust's `char::is_control`), C1 included.
    for (const c of ["\u0000", "\u007f", "\u0080", "\u0085", "\u009f"]) expect(safeHref(`https://x.test/${c}`)).toBe(false);
    expect(safeHref("https://x.test/\u00a9")).toBe(true);
  });

  it("render as elements, never as injected HTML", () => {
    const doc = asDocument({ type: "doc", version: 1, content: [
      { type: "paragraph", content: [{ type: "text", text: "<img src=x onerror=alert(1)>" }] },
      { type: "paragraph", content: [{ type: "text", text: "go", marks: [{ type: "link", attrs: { href: "javascript:alert(1)" } }] }] },
      { type: "image", attrs: { media: ID, alt: "cat" } },
    ] })!;
    const { container } = render(<Providers><RichTextView doc={doc} /></Providers>);
    expect(screen.getByText("<img src=x onerror=alert(1)>")).toBeInTheDocument();
    expect(container.querySelectorAll("img")).toHaveLength(1);
    expect(container.querySelector("img")!.getAttribute("src")).toBe(`${API}/media/${ID}/file`);
    expect(container.querySelector("a")).toBeNull();
  });

  it("show an unavailable chip, not a broken image, for files that cannot be loaded", () => {
    const doc = asDocument({ type: "doc", version: 1, content: [{ type: "image", attrs: { media: ID, alt: "cat" } }] })!;
    const { container } = render(<Providers><RichTextView doc={doc} /></Providers>);
    fireEvent.error(container.querySelector("img")!);
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByRole("img", { name: "File unavailable" })).toHaveTextContent("File unavailable");
  });
});

describe("file fields", () => {
  it("show the same chip for files that cannot be loaded, and a link for other files", async () => {
    const OTHER = "0199d3a4-1b2c-7d3e-8f40-123456789abe";
    server.use(
      http.head(`*/admin/api/media/${ID}/file`, () => new HttpResponse(null, { status: 404 })),
      http.head(`*/admin/api/media/${OTHER}/file`, () => new HttpResponse(null, { status: 200 })),
    );
    const { container } = render(<Providers><MediaThumb id={ID} /><MediaThumb id={OTHER} /></Providers>);
    for (const img of container.querySelectorAll("img")) fireEvent.error(img);
    // Missing or trashed: the chip; a saved PDF (not an image): its download link.
    expect(await screen.findByRole("img", { name: "File unavailable" })).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "File" })).toHaveAttribute("href", `${API}/media/${OTHER}/file`);
    server.resetHandlers();
  });
});

describe("rich text images", () => {
  it("are tried again when their source changes", () => {
    const doc = (id: string) => asDocument({ type: "doc", version: 1, content: [{ type: "image", attrs: { media: id, alt: "cat" } }] })!;
    const { container, rerender } = render(<Providers><RichTextView doc={doc(ID)} /></Providers>);
    fireEvent.error(container.querySelector("img")!);
    expect(container.querySelector("img")).toBeNull();
    const OTHER = "0199d3a4-1b2c-7d3e-8f40-123456789abe";
    rerender(<Providers><RichTextView doc={doc(OTHER)} /></Providers>);
    expect(container.querySelector("img")?.getAttribute("src")).toBe(`${API}/media/${OTHER}/file`);
  });
});

const input = (overrides: Partial<InputSchema> = {}): InputSchema => ({
  name: "photo", label: "Photo", type: "file_upload", valueType: "uuid", nullable: true, disabled: false,
  rules: [], columnSpan: 1, media: { multiple: false, maxItems: 1, accept: ["image/*"] }, ...overrides,
});

function UploadHarness() {
  const [value, setValue] = useState<string | string[]>("");
  return (
    <Providers>
      <FileUploadWidget id="field-photo" input={input()} value={value} onChange={setValue} invalid={false} />
      <output data-testid="value">{JSON.stringify(value)}</output>
    </Providers>
  );
}

describe("file upload", () => {
  it("uploads, and retries an unknown outcome with the same key", async () => {
    // Node's fetch cannot send jsdom's FormData: answer at the fetch boundary.
    const real = globalThis.fetch;
    vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
      if (!url.endsWith("/media")) return real(url, init);
      expect(init.body).toBeInstanceOf(FormData);
      expect((init.headers as Record<string, string>)["Content-Type"]).toBeUndefined();
      keys.push((init.headers as Record<string, string>)["Idempotency-Key"] ?? null);
      if (keys.length === 1) return new Response(null, { status: 504 });
      const data = { id: ID, name: "cat.png", mime: "image/png", size: 3, inline: true };
      return new Response(JSON.stringify({ data }), { status: 201, headers: { "Content-Type": "application/json" } });
    });
    const { container } = render(<UploadHarness />);
    const picker = container.querySelector("input[type=file]") as HTMLInputElement;
    await userEvent.upload(picker, new File(["png"], "cat.png", { type: "image/png" }));
    await userEvent.click(await screen.findByRole("button", { name: "Retry upload" }));
    await waitFor(() => expect(screen.getByTestId("value").textContent).toBe(JSON.stringify(ID)));
    expect(keys).toHaveLength(2);
    expect(keys[1]).toBe(keys[0]);
    vi.unstubAllGlobals();
  });

  it("refuses files the field does not accept before sending", async () => {
    const { container } = render(<UploadHarness />);
    const picker = container.querySelector("input[type=file]") as HTMLInputElement;
    await userEvent.upload(picker, new File(["%PDF"], "doc.pdf", { type: "application/pdf" }), { applyAccept: false });
    expect(await screen.findByText(/not accepted/)).toBeInTheDocument();
    expect(keys).toHaveLength(0);
  });
});

function ListHarness() {
  const [value, setValue] = useState<string | string[]>([ID, "0199d3a4-1b2c-7d3e-8f40-123456789abd"]);
  return (
    <Providers>
      <FileUploadWidget id="field-gallery" input={input({ name: "gallery", media: { multiple: true, maxItems: 3, accept: [] } })} value={value} onChange={setValue} invalid={false} />
      <output data-testid="value">{JSON.stringify(value)}</output>
    </Providers>
  );
}

describe("media lists", () => {
  it("reorder existing items", async () => {
    render(<ListHarness />);
    await userEvent.click(screen.getByRole("button", { name: "Move file 2 earlier" }));
    expect(JSON.parse(screen.getByTestId("value").textContent!)).toEqual(["0199d3a4-1b2c-7d3e-8f40-123456789abd", ID]);
  });
});

describe("rich editor", () => {
  it("mounts, edits and emits a document", async () => {
    let emitted = "";
    render(
      <Providers>
        <RichEditorWidget id="field-body" label="Body" value="" onChange={(v) => (emitted = v)} invalid={false} nullable />
      </Providers>,
    );
    const box = await screen.findByRole("textbox", { name: "Body" });
    await userEvent.click(box);
    await userEvent.keyboard("Hello");
    await waitFor(() => expect(emitted).not.toBe(""));
    expect(JSON.parse(emitted)).toEqual({ type: "doc", version: 1, content: [{ type: "paragraph", content: [{ type: "text", text: "Hello" }] }] });
    expect(screen.getByRole("toolbar", { name: "Body formatting" })).toBeInTheDocument();
  });

  it("adds, checks, edits and removes links in the panel's dialog (no browser prompt)", async () => {
    const prompt = vi.spyOn(window, "prompt");
    let emitted = "";
    const doc = (text: string) => JSON.stringify({ type: "doc", version: 1, content: [{ type: "paragraph", content: [{ type: "text", text }] }] });
    render(
      <Providers>
        <form onSubmit={() => { throw new Error("the record form was submitted"); }}>
          <RichEditorWidget id="field-body" label="Body" value={doc("Read the docs")} onChange={(v) => (emitted = v)} invalid={false} nullable />
        </form>
      </Providers>,
    );
    const box = await screen.findByRole("textbox", { name: "Body" });
    await userEvent.click(box);
    await userEvent.keyboard("{Control>}a{/Control}");
    await userEvent.click(screen.getByRole("button", { name: "Link" }));
    let dialog = await screen.findByRole("dialog", { name: "Add link" });
    const address = within(dialog).getByLabelText("Link address");
    // Enter confirming an IME composition does not apply the address.
    await userEvent.type(address, "https://ví");
    fireEvent.keyDown(address, { key: "Enter", isComposing: true });
    expect(screen.getByRole("dialog", { name: "Add link" })).toBeInTheDocument();
    await userEvent.clear(address);
    // Checked like the server: no `javascript:`; Enter never submits the record form.
    await userEvent.type(address, "javascript:alert(1){Enter}");
    expect(within(dialog).getByRole("alert")).toHaveTextContent("Links must start with https://, http:// or mailto:.");
    await userEvent.clear(address);
    await userEvent.type(address, "https://example.com{Enter}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Add link" })).toBeNull());
    const marks = () => JSON.parse(emitted).content[0].content[0].marks;
    await waitFor(() => expect(marks()).toEqual([{ type: "link", attrs: { href: "https://example.com" } }]));

    // On the link: the dialog edits it, and can remove it.
    await userEvent.click(box);
    await userEvent.keyboard("{Control>}a{/Control}");
    await userEvent.click(screen.getByRole("button", { name: "Link" }));
    dialog = await screen.findByRole("dialog", { name: "Edit link" });
    expect(within(dialog).getByLabelText("Link address")).toHaveValue("https://example.com");
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove link" }));
    await waitFor(() => expect(marks()).toBeUndefined());
    expect(prompt).not.toHaveBeenCalled();
  });
});
