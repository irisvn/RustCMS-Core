import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { SessionContext } from "@/api/context";
import type { InputSchema } from "@/api/types";
import { createSession } from "@/app/session";
import { setLocale } from "@/i18n";
import { FileUploadWidget } from "@/renderers/media";
import { RichEditorWidget } from "@/renderers/rich-text";
import { mimeFilter } from "@/renderers/media-picker";

configure({ asyncUtilTimeout: 4000 });
vi.setConfig({ testTimeout: 20_000 });

const API = "http://localhost/admin/api";
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const idOf = (n: number) => `0199d3a4-1b2c-7d3e-8f40-${String(n).padStart(12, "0")}`;

interface Row { n: number; name: string; mime: string; size: number }

/**
 * The library: `rows` in scope, 24 a page; records every list query (answered
 * once `hold` resolves) and every upload (stored at once, or refused with
 * 429 the first `busy[name]` times).
 */
function library(rows: Row[], { permissions = ["media.view_any", "media.view", "media.create"], uploads = true, hold = Promise.resolve(), busy = {} as Record<string, number> } = {}) {
  const queries: URLSearchParams[] = [];
  const sent: string[] = [];
  vi.stubGlobal("fetch", async (url: string, init?: RequestInit) => {
    const u = new URL(url, "http://localhost");
    if (u.pathname.endsWith("/panel")) {
      return json({ brand: "x", user: { id: "1", name: "a", email: "a@b.c" }, navigation: [], system: {}, settings: [], pages: [], permissions, ...(uploads ? { uploads: { maxBytes: 1000, maxConcurrent: 2 } } : {}) });
    }
    if (u.pathname.endsWith("/auth/csrf")) return json({ csrfToken: "t", userId: "1" });
    if (u.pathname.endsWith("/media") && init?.method === "POST") {
      const file = (init.body as FormData).get("file") as File;
      sent.push(file.name);
      // The server's upload limit is reached: try again in a second.
      if ((busy[file.name] ?? 0) > 0) {
        busy[file.name]! -= 1;
        return new Response(null, { status: 429, headers: { "Retry-After": "1" } });
      }
      return json({ data: { id: idOf(100 + sent.length), name: file.name, mime: file.type, size: file.size, inline: true } }, 201);
    }
    if (u.pathname.endsWith("/resources/media/records")) {
      queries.push(u.searchParams);
      await hold;
      const search = u.searchParams.get("search") ?? "";
      const mimes = u.searchParams.get("filter[mime][in]")?.split(",");
      const page = Number(u.searchParams.get("page"));
      const found = rows.filter((r) => r.name.includes(search) && (!mimes || mimes.includes(r.mime)));
      const data = found.slice((page - 1) * 24, page * 24).map((r) => ({
        key: idOf(r.n), title: r.name, abilities: {},
        // i64 columns are strings on the wire.
        data: { name: r.name, mime: r.mime, size: String(r.size), width: 10, height: 10, owner_id: "1" },
      }));
      return json({ data, meta: { page, perPage: 24, total: found.length, lastPage: Math.max(1, Math.ceil(found.length / 24)) } });
    }
    throw new Error(`unexpected ${url}`);
  });
  return { queries, sent };
}

const input = (media: InputSchema["media"]): InputSchema => ({
  name: "gallery", label: "Gallery", type: "file_upload", valueType: "json", nullable: true, disabled: false, rules: [], columnSpan: 1, media,
});

function Field({ media, initial }: { media: InputSchema["media"]; initial: string | string[] }) {
  const [value, setValue] = useState<string | string[]>(initial);
  const client = new QueryClient();
  const session = createSession({ basePath: "/admin", apiPath: API }, client, () => {});
  return (
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={client}>
        <FileUploadWidget id="field-gallery" input={input(media)} value={value} onChange={setValue} invalid={false} />
        <output data-testid="value">{JSON.stringify(value)}</output>
      </QueryClientProvider>
    </SessionContext.Provider>
  );
}

const value = () => JSON.parse(screen.getByTestId("value").textContent!);

afterEach(() => {
  vi.unstubAllGlobals();
  setLocale("en");
});

describe("media library picker", () => {
  it("lists the accepted files in scope with thumbnails, searches and pages", async () => {
    const rows: Row[] = [
      ...Array.from({ length: 30 }, (_, i) => ({ n: i + 1, name: `cat-${i + 1}.png`, mime: "image/png", size: 100 })),
      { n: 99, name: "cat.pdf", mime: "application/pdf", size: 100 },
    ];
    const { queries } = library(rows);
    render(<Field media={{ multiple: true, maxItems: 5, accept: ["image/*"] }} initial={[]} />);
    const open = await screen.findByRole("button", { name: "Media library" });
    await waitFor(() => expect(open).toBeEnabled());
    await userEvent.click(open);
    const dialog = await screen.findByRole("dialog", { name: "Gallery" });
    expect(within(dialog).getByRole("tab", { name: "Library" })).toHaveAttribute("aria-selected", "true");
    // The search box has the focus.
    expect(within(dialog).getByLabelText("Search by name")).toHaveFocus();
    const items = await within(dialog).findByRole("list", { name: "Files" });
    expect(within(items).getAllByRole("button")).toHaveLength(24);
    // Thumbnails: the authorized file URL, scaled by the browser.
    const img = items.querySelector("img")!;
    expect(img.getAttribute("src")).toBe(`${API}/media/${idOf(1)}/file`);
    expect(dialog.querySelector("[style]")).toBeNull();
    const first = queries[0]!;
    expect(first.get("filter[mime][in]")).toBe("image/png,image/jpeg,image/gif,image/webp");
    expect([first.get("page"), first.get("per_page"), first.get("sort")]).toEqual(["1", "24", "-created_at"]);
    // Pages.
    expect(within(dialog).getByText("Page 1 of 2")).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Next" }));
    await within(dialog).findByText("Page 2 of 2");
    // The previous page stays shown until the next one is in.
    await waitFor(() => expect(within(within(dialog).getByRole("list", { name: "Files" })).getAllByRole("button")).toHaveLength(6));
    // Search by name: back to the first page.
    await userEvent.type(within(dialog).getByLabelText("Search by name"), "cat-3");
    await waitFor(() => expect(queries.at(-1)!.get("search")).toBe("cat-3"));
    expect(queries.at(-1)!.get("page")).toBe("1");
    await waitFor(() => expect(within(within(dialog).getByRole("list", { name: "Files" })).getAllByRole("button")).toHaveLength(2));
  });

  it("chooses with the keyboard within the field's rules and adds to the value", async () => {
    library([
      { n: 1, name: "a.png", mime: "image/png", size: 100 },
      { n: 2, name: "b.png", mime: "image/png", size: 100 },
      { n: 3, name: "big.png", mime: "image/png", size: 5000 },
      { n: 4, name: "c.png", mime: "image/png", size: 100 },
      { n: 5, name: "d.png", mime: "image/png", size: 100 },
    ]);
    render(<Field media={{ multiple: true, maxItems: 3, accept: ["image/*"], maxSize: 1000 }} initial={[idOf(4)]} />);
    const open = await screen.findByRole("button", { name: "Media library" });
    await waitFor(() => expect(open).toBeEnabled());
    await userEvent.click(open);
    const dialog = await screen.findByRole("dialog", { name: "Gallery" });
    const items = await within(dialog).findByRole("list", { name: "Files" });
    const button = (name: string) => within(items).getByRole("button", { name: new RegExp(`^${name}`) });
    // Too large for the field, or already in it: not offered.
    expect(button("big.png")).toBeDisabled();
    expect(button("big.png")).toHaveAccessibleDescription("Too large for this field");
    expect(button("c.png")).toBeDisabled();
    expect(button("c.png")).toHaveAccessibleDescription("Already used");
    // Two slots left (3 at most, one used).
    button("a.png").focus();
    await userEvent.keyboard(" ");
    expect(button("a.png")).toHaveAttribute("aria-pressed", "true");
    await userEvent.tab();
    expect(button("b.png")).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    expect(within(dialog).getByText("2 of at most 2 chosen")).toBeInTheDocument();
    // Full: no more can be chosen (the chosen ones can still be unchosen).
    expect(button("d.png")).toBeDisabled();
    expect(button("b.png")).toBeEnabled();
    await userEvent.click(within(dialog).getByRole("button", { name: "Use" }));
    await waitFor(() => expect(value()).toEqual([idOf(4), idOf(1), idOf(2)]));
    expect(screen.queryByRole("dialog", { name: "Gallery" })).toBeNull();
  });

  it("replaces a single file's value, and is in Vietnamese", async () => {
    setLocale("vi");
    library([{ n: 1, name: "a.png", mime: "image/png", size: 100 }, { n: 2, name: "b.pdf", mime: "application/pdf", size: 100 }]);
    render(<Field media={{ multiple: false, maxItems: 1, accept: [] }} initial={idOf(9)} />);
    const open = await screen.findByRole("button", { name: "Thư viện tệp" });
    await waitFor(() => expect(open).toBeEnabled());
    await userEvent.click(open);
    const dialog = await screen.findByRole("dialog", { name: "Gallery" });
    const items = await within(dialog).findByRole("list", { name: "Tệp" });
    // Any type: the PDF is offered, as an icon (no image request for it).
    expect(items.querySelectorAll("img")).toHaveLength(1);
    await userEvent.click(within(items).getByRole("button", { name: /b\.pdf/ }));
    await userEvent.click(within(items).getByRole("button", { name: /a\.png/ }));
    expect(within(items).getByRole("button", { name: /b\.pdf/ })).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(within(dialog).getByRole("button", { name: "Dùng" }));
    await waitFor(() => expect(value()).toBe(idOf(1)));
  });

  it("says the library is loading until its first page is in", async () => {
    let release = () => {};
    library([{ n: 1, name: "a.png", mime: "image/png", size: 100 }], { hold: new Promise<void>((r) => (release = r)) });
    render(<Field media={{ multiple: true, maxItems: 3, accept: ["image/*"] }} initial={[]} />);
    const open = await screen.findByRole("button", { name: "Media library" });
    await waitFor(() => expect(open).toBeEnabled());
    await userEvent.click(open);
    const dialog = await screen.findByRole("dialog", { name: "Gallery" });
    expect(await within(dialog).findByRole("status")).toHaveTextContent("Loading the media library…");
    expect(within(dialog).queryByText("No files.")).toBeNull();
    release();
    await within(dialog).findByRole("list", { name: "Files" });
    expect(within(dialog).queryByText("Loading the media library…")).toBeNull();
  });

  it("uploads no more files than can still be chosen, and says so", async () => {
    const { sent } = library([]);
    // Three at most, one used: two more.
    render(<Field media={{ multiple: true, maxItems: 3, accept: ["image/*"] }} initial={[idOf(4)]} />);
    const open = await screen.findByRole("button", { name: "Media library" });
    await waitFor(() => expect(open).toBeEnabled());
    await userEvent.click(open);
    const dialog = await screen.findByRole("dialog", { name: "Gallery" });
    await userEvent.click(within(dialog).getByRole("tab", { name: "Upload" }));
    const picker = dialog.querySelector("input[type=file]") as HTMLInputElement;
    const png = (name: string) => new File(["x"], name, { type: "image/png" });
    const pdf = new File(["%PDF"], "doc.pdf", { type: "application/pdf" });
    // The refused PDF takes no place; the third image is left out.
    await userEvent.upload(picker, [png("p1.png"), pdf, png("p2.png"), png("p3.png")], { applyAccept: false });
    expect(within(dialog).getByText("Only 2 more files can be chosen: the others were not added.")).toBeInTheDocument();
    expect(within(dialog).getByText("doc.pdf: this kind of file is not accepted.")).toBeInTheDocument();
    await within(dialog).findByText("2 of at most 2 chosen");
    expect(sent).toEqual(["p1.png", "p2.png"]);
    // Full: nothing more is sent.
    await userEvent.upload(picker, [png("p4.png")]);
    expect(within(dialog).getByText("No more files can be chosen: they were not added.")).toBeInTheDocument();
    await new Promise((r) => setTimeout(r, 50));
    expect(sent).toEqual(["p1.png", "p2.png"]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Use" }));
    await waitFor(() => expect(value()).toEqual([idOf(4), idOf(101), idOf(102)]));
  });

  it("keeps room for files in but held back for the ones before them", async () => {
    const { sent } = library([], { busy: { "a.png": 1 } });
    render(<Field media={{ multiple: true, maxItems: 2, accept: ["image/*"] }} initial={[]} />);
    const open = await screen.findByRole("button", { name: "Media library" });
    await waitFor(() => expect(open).toBeEnabled());
    await userEvent.click(open);
    const dialog = await screen.findByRole("dialog", { name: "Gallery" });
    await userEvent.click(within(dialog).getByRole("tab", { name: "Upload" }));
    const picker = dialog.querySelector("input[type=file]") as HTMLInputElement;
    const png = (name: string) => new File(["x"], name, { type: "image/png" });
    // a.png waits for its retry; b.png is in, held back until a.png is.
    await userEvent.upload(picker, [png("a.png"), png("b.png")]);
    await within(dialog).findByText("The server is busy: trying again in 1 s…");
    await waitFor(() => expect(sent).toEqual(["a.png", "b.png"]));
    await userEvent.upload(picker, [png("c.png")]);
    expect(within(dialog).getByText("No more files can be chosen: they were not added.")).toBeInTheDocument();
    await within(dialog).findByText("2 of at most 2 chosen");
    expect(sent).toEqual(["a.png", "b.png", "a.png"]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Use" }));
    await waitFor(() => expect(value()).toEqual([idOf(103), idOf(102)]));
  });

  it("gives no room to a file over the server's size limit", async () => {
    const { sent } = library([]);
    // One more at most; the server takes 1,000 bytes at most (the field sets no size).
    render(<Field media={{ multiple: true, maxItems: 2, accept: ["image/*"] }} initial={[idOf(4)]} />);
    const open = await screen.findByRole("button", { name: "Media library" });
    await waitFor(() => expect(open).toBeEnabled());
    await userEvent.click(open);
    const dialog = await screen.findByRole("dialog", { name: "Gallery" });
    await userEvent.click(within(dialog).getByRole("tab", { name: "Upload" }));
    const picker = dialog.querySelector("input[type=file]") as HTMLInputElement;
    await userEvent.upload(picker, [new File(["x".repeat(2000)], "big.png", { type: "image/png" }), new File(["x"], "small.png", { type: "image/png" })]);
    expect(within(dialog).getByText("big.png is too large.")).toBeInTheDocument();
    expect(within(dialog).queryByText(/can be chosen/)).toBeNull();
    await within(dialog).findByText("1 of at most 1 chosen");
    expect(sent).toEqual(["small.png"]);
  });

  it("offers what the user may: the library with media.view_any, uploads with media.create", async () => {
    library([], { permissions: ["media.view_any", "media.create"], uploads: true });
    render(<Field media={{ multiple: false, maxItems: 1, accept: [] }} initial="" />);
    // `media.view_any` without `media.view`: no library button in the field (it still uploads directly).
    await screen.findByRole("button", { name: "Upload file" });
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByRole("button", { name: "Media library" })).toBeNull();
  });
});

describe("editor image picker", () => {
  it("inserts library images in the order chosen, images only", async () => {
    const { queries } = library([{ n: 1, name: "a.png", mime: "image/png", size: 1 }, { n: 2, name: "b.png", mime: "image/png", size: 1 }]);
    let emitted = "";
    const client = new QueryClient();
    const session = createSession({ basePath: "/admin", apiPath: API }, client, () => {});
    render(
      <SessionContext.Provider value={session}>
        <QueryClientProvider client={client}>
          <RichEditorWidget id="field-body" label="Body" value="" onChange={(v) => (emitted = v)} invalid={false} nullable />
        </QueryClientProvider>
      </SessionContext.Provider>,
    );
    await userEvent.click(await screen.findByRole("textbox", { name: "Body" }));
    await userEvent.keyboard("Hi");
    const button = screen.getByRole("button", { name: "Insert image" });
    await waitFor(() => expect(button).toBeEnabled());
    await userEvent.click(button);
    const dialog = await screen.findByRole("dialog", { name: "Insert images" });
    const items = await within(dialog).findByRole("list", { name: "Files" });
    expect(queries[0]!.get("filter[mime][in]")).toBe("image/png,image/jpeg,image/gif,image/webp");
    await userEvent.click(within(items).getByRole("button", { name: /b\.png/ }));
    await userEvent.click(within(items).getByRole("button", { name: /a\.png/ }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(emitted).not.toBe(""));
    const content = JSON.parse(emitted).content as { type: string; attrs?: { media?: string } }[];
    expect(content.map((n) => n.type)).toEqual(["paragraph", "image", "image", "paragraph"]);
    expect(content.filter((n) => n.type === "image").map((n) => n.attrs?.media)).toEqual([idOf(2), idOf(1)]);
  });
});

describe("mime filters", () => {
  it("expand accept lists to the stored types", () => {
    expect(mimeFilter([])).toBeNull();
    expect(mimeFilter(["image/*"])).toBe("image/png,image/jpeg,image/gif,image/webp");
    expect(mimeFilter(["application/pdf", "image/png"])).toBe("image/png,application/pdf");
    expect(mimeFilter(["video/*"])).toBe("none/none");
  });
});
