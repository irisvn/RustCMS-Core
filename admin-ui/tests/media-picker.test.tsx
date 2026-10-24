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

/** The library: `rows` in scope, 24 a page; records every list query. */
function library(rows: Row[], { permissions = ["media.view_any", "media.view", "media.create"], uploads = true } = {}) {
  const queries: URLSearchParams[] = [];
  vi.stubGlobal("fetch", async (url: string) => {
    const u = new URL(url, "http://localhost");
    if (u.pathname.endsWith("/panel")) {
      return json({ brand: "x", user: { id: "1", name: "a", email: "a@b.c" }, navigation: [], system: {}, settings: [], pages: [], permissions, ...(uploads ? { uploads: { maxBytes: 1000, maxConcurrent: 2 } } : {}) });
    }
    if (u.pathname.endsWith("/resources/media/records")) {
      queries.push(u.searchParams);
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
  return { queries };
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
    expect(within(within(dialog).getByRole("list", { name: "Files" })).getAllByRole("button")).toHaveLength(6);
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
