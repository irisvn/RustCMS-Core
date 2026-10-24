import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SessionContext } from "@/api/context";
import { createSession } from "@/app/session";
import { type DocNode, asDocument, isEmptyDocument, parseEmbedUrl, toDocument, toEditor } from "@/lib/rich-text";
import { RichEditorWidget, RichTextView } from "@/renderers/rich-text";

const API = "http://localhost/admin/api";
const ID = "0199d3a4-1b2c-7d3e-8f40-123456789abc";
const SRC = `${API}/media/${ID}/file`;

function Providers({ children }: { children: React.ReactNode }) {
  const client = new QueryClient();
  const session = createSession({ basePath: "/admin", apiPath: API }, client, () => {});
  return (
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    </SessionContext.Provider>
  );
}

const p = (text?: string): DocNode => (text ? { type: "paragraph", content: [{ type: "text", text }] } : { type: "paragraph", content: [] });
const cell = (type: string, text?: string): DocNode => ({ type, content: [p(text)] });

/** An editor on `doc`; returns a getter of the last emitted document. */
async function editorOn(content: DocNode[] | null) {
  let emitted = "";
  const value = content ? JSON.stringify({ type: "doc", version: 2, content }) : "";
  render(
    <Providers>
      <RichEditorWidget id="field-body" label="Body" value={value} onChange={(v) => (emitted = v)} invalid={false} nullable />
    </Providers>,
  );
  const box = await screen.findByRole("textbox", { name: "Body" });
  await userEvent.click(box);
  return { box, doc: () => (emitted ? (JSON.parse(emitted) as { version: number; content: DocNode[] }) : null) };
}

describe("version 2 documents", () => {
  it("map captions, widths, tables, embeds and callouts, and save version 2", () => {
    const editor: DocNode = {
      type: "doc",
      content: [
        { type: "image", attrs: { src: SRC, alt: "a", caption: "  ", size: "normal", title: "x" } },
        { type: "image", attrs: { src: SRC, alt: "b", caption: "Cap", size: "full" } },
        { type: "image", attrs: { src: SRC, alt: "c", caption: "😀".repeat(1200), size: "huge" } },
        // Spans become empty cells, short rows are padded, header cells only in a header row.
        { type: "table", content: [
          { type: "tableRow", content: [{ type: "tableHeader", attrs: { colspan: 2, rowspan: 1, colwidth: [40] }, content: [p("H")] }] },
          { type: "tableRow", content: [{ type: "tableHeader", content: [p("x")] }, { type: "tableCell", content: [p("y")] }] },
          { type: "tableRow", content: [{ type: "tableCell", content: [p("z")] }] },
        ] },
        { type: "table", content: [{ type: "tableRow", content: [cell("tableHeader", "a"), cell("tableCell", "b")] }] },
        { type: "table", content: [] },
        { type: "embed", attrs: { provider: "youtube", id: "dQw4w9WgXcQ" } },
        { type: "embed", attrs: { provider: "evil", id: "x" } },
        { type: "embed", attrs: { provider: "youtube", id: "<script>" } },
        { type: "callout", attrs: { tone: "danger" }, content: [p("c")] },
      ],
    };
    const doc = toDocument(editor, API);
    expect(doc.version).toBe(2);
    expect(doc.content).toEqual([
      { type: "image", attrs: { media: ID, alt: "a", caption: "  " } },
      { type: "image", attrs: { media: ID, alt: "b", caption: "Cap", width: "full" } },
      { type: "image", attrs: { media: ID, alt: "c", caption: "😀".repeat(1000) } },
      { type: "table", content: [
        { type: "tableRow", content: [cell("tableHeader", "H"), cell("tableHeader")] },
        { type: "tableRow", content: [cell("tableCell", "x"), cell("tableCell", "y")] },
        { type: "tableRow", content: [cell("tableCell", "z"), cell("tableCell")] },
      ] },
      { type: "table", content: [{ type: "tableRow", content: [cell("tableCell", "a"), cell("tableCell", "b")] }] },
      { type: "embed", attrs: { provider: "youtube", id: "dQw4w9WgXcQ" } },
      { type: "callout", attrs: { tone: "info" }, content: [p("c")] },
    ]);
    // Read back: the editor gets the same attributes.
    const back = toEditor(doc, API);
    expect(back.content?.[1]?.attrs).toEqual({ src: SRC, alt: "b", caption: "Cap", size: "full" });
    expect(toDocument(back, API)).toEqual(doc);
    expect(isEmptyDocument({ type: "doc", version: 2, content: [{ type: "embed", attrs: { provider: "vimeo", id: "1" } }] })).toBe(false);
  });

  it("are edited when the editor can hold them, version 1 as it is", () => {
    const table = { type: "table", content: [{ type: "tableRow", content: [cell("tableHeader", "a")] }, { type: "tableRow", content: [cell("tableCell", "b")] }] };
    const v2 = [table, { type: "embed", attrs: { provider: "x", id: "123" } }, { type: "callout", attrs: { tone: "warn" }, content: [p("w")] },
      { type: "image", attrs: { media: ID, alt: "", caption: "c", width: "wide" } }];
    expect(asDocument({ type: "doc", version: 2, content: v2 })).not.toBeNull();
    expect(asDocument({ type: "doc", version: 1, content: [{ type: "image", attrs: { media: ID, alt: "x" } }] })).not.toBeNull();
    // Version 2 parts in a version 1 document, or shapes the server refuses: shown read-only.
    for (const node of v2) expect(asDocument({ type: "doc", version: 1, content: [node] }), JSON.stringify(node)).toBeNull();
    for (const node of [
      { type: "table", content: [{ type: "tableRow", content: [cell("tableCell", "a"), cell("tableCell", "b")] }, { type: "tableRow", content: [cell("tableCell", "c")] }] },
      { type: "table", content: [{ type: "tableRow", content: [cell("tableCell", "a")] }, { type: "tableRow", content: [cell("tableHeader", "b")] }] },
      { type: "table", content: [{ type: "tableRow", content: [{ type: "tableCell", attrs: { colspan: 2 }, content: [p("a")] }] }] },
      { type: "table", content: [{ type: "tableRow", content: [{ type: "tableCell", content: [{ type: "bulletList", content: [] }] }] }] },
      { type: "embed", attrs: { provider: "youtube", id: "https://youtu.be/dQw4w9WgXcQ" } },
      { type: "embed", attrs: { provider: "dailymotion", id: "x7tgad0" } },
      { type: "callout", attrs: { tone: "danger" }, content: [p("x")] },
      { type: "callout", attrs: { tone: "info" }, content: [] },
      { type: "image", attrs: { media: ID, width: "huge" } },
      { type: "doc", version: 3, content: [] },
    ]) {
      expect(asDocument({ type: "doc", version: 2, content: [node] }), JSON.stringify(node)).toBeNull();
    }
    expect(asDocument({ type: "doc", version: 3, content: [] })).toBeNull();
  });
});

describe("embed addresses", () => {
  it("keep the provider and the id of allowed addresses only", () => {
    const yt = { provider: "youtube", id: "dQw4w9WgXcQ" };
    for (const url of [
      "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
      "https://youtube.com/watch?v=dQw4w9WgXcQ&t=42s",
      "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
      "https://youtu.be/dQw4w9WgXcQ?si=abc",
      "https://www.youtube.com/embed/dQw4w9WgXcQ",
      "https://www.youtube.com/shorts/dQw4w9WgXcQ",
      "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
      "  https://www.youtube.com/live/dQw4w9WgXcQ  ",
    ]) expect(parseEmbedUrl(url), url).toEqual(yt);
    expect(parseEmbedUrl("https://vimeo.com/76979871")).toEqual({ provider: "vimeo", id: "76979871" });
    expect(parseEmbedUrl("https://player.vimeo.com/video/76979871?h=x")).toEqual({ provider: "vimeo", id: "76979871" });
    expect(parseEmbedUrl("https://x.com/rustlang/status/1460323737035677698")).toEqual({ provider: "x", id: "1460323737035677698" });
    expect(parseEmbedUrl("https://twitter.com/i/web/status/1460323737035677698/photo/1")).toEqual({ provider: "x", id: "1460323737035677698" });
    for (const url of [
      "javascript:alert(1)//youtube.com/watch?v=dQw4w9WgXcQ",
      "data:text/html,https://youtu.be/dQw4w9WgXcQ",
      "https://youtube.com.evil.test/watch?v=dQw4w9WgXcQ",
      "https://evil.test/youtube.com/watch?v=dQw4w9WgXcQ",
      "https://user:pw@youtube.com/watch?v=dQw4w9WgXcQ",
      "https://youtube.com:8443/watch?v=dQw4w9WgXcQ",
      "https://www.youtube.com/watch?v=dQw4w9WgXc",
      "https://www.youtube.com/watch?v=dQw4w9WgXcQ%22%3E",
      "https://www.youtube.com/playlist?list=PL123",
      "https://youtu.be/",
      "https://vimeo.com/channels/staffpicks",
      "https://vimeo.com/76979871a",
      "https://x.com/status/1",
      "https://x.com/a/b/status/1",
      "https://x.com/rustlang",
      "ftp://youtu.be/dQw4w9WgXcQ",
      "not a url",
    ]) expect(parseEmbedUrl(url), url).toBeNull();
  });
});

describe("rich editor (version 2)", () => {
  it("inserts a table with a header row, grows it within the limits and removes it", async () => {
    const { doc } = await editorOn(null);
    await userEvent.keyboard("Intro");
    await userEvent.click(screen.getByRole("button", { name: "Insert table" }));
    const tools = await screen.findByRole("toolbar", { name: "Table" });
    expect(within(tools).getByText("3 × 3 (at most 50 × 20)")).toBeInTheDocument();
    await waitFor(() => expect(doc()?.content[1]?.type).toBe("table"));
    const table = doc()!.content[1]!;
    expect(table.content).toHaveLength(3);
    expect(table.content![0]!.content!.map((c) => c.type)).toEqual(["tableHeader", "tableHeader", "tableHeader"]);
    expect(table.content![1]!.content![0]).toEqual({ type: "tableCell", content: [{ type: "paragraph", content: [] }] });
    await userEvent.keyboard("Name");
    await userEvent.click(within(tools).getByRole("button", { name: "Add row" }));
    await userEvent.click(within(tools).getByRole("button", { name: "Add column" }));
    await waitFor(() => expect(doc()!.content[1]!.content).toHaveLength(4));
    expect(doc()!.content[1]!.content![0]!.content).toHaveLength(4);
    expect(doc()!.content[1]!.content![0]!.content![0]).toEqual(cell("tableHeader", "Name"));
    expect(doc()!.version).toBe(2);
    await userEvent.click(within(tools).getByRole("button", { name: "Remove table" }));
    await waitFor(() => expect(doc()!.content.some((n) => n.type === "table")).toBe(false));
    expect(screen.queryByRole("toolbar", { name: "Table" })).toBeNull();
  });

  it("does not grow a table past 50 rows or 20 columns", async () => {
    const row = (n: number) => ({ type: "tableRow", content: Array.from({ length: n }, () => cell("tableCell", "x")) });
    const { box } = await editorOn([{ type: "table", content: Array.from({ length: 50 }, () => row(20)) }]);
    await userEvent.click(box.querySelector("td p")!);
    // 1,000 cells: slow to render in jsdom.
    const tools = await screen.findByRole("toolbar", { name: "Table" }, { timeout: 5000 });
    expect(within(tools).getByRole("button", { name: "Add row" })).toBeDisabled();
    expect(within(tools).getByRole("button", { name: "Add column" })).toBeDisabled();
    expect(within(tools).getByRole("button", { name: "Remove row" })).toBeEnabled();
  });

  it("embeds an allowed address as {provider, id}, shown as a card without iframe or image", async () => {
    const { box, doc } = await editorOn(null);
    await userEvent.click(screen.getByRole("button", { name: "Embed a video or post" }));
    const dialog = await screen.findByRole("dialog", { name: "Embed a video or post" });
    const address = within(dialog).getByLabelText("Address");
    await userEvent.type(address, "https://evil.test/watch?v=dQw4w9WgXcQ{Enter}");
    expect(within(dialog).getByRole("alert")).toHaveTextContent("Only YouTube, Vimeo and X");
    await userEvent.clear(address);
    await userEvent.type(address, "https://youtu.be/dQw4w9WgXcQ?t=1");
    expect(within(dialog).getByText("https://www.youtube.com/watch?v=dQw4w9WgXcQ")).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(doc()?.content.find((n) => n.type === "embed")).toEqual({ type: "embed", attrs: { provider: "youtube", id: "dQw4w9WgXcQ" } }));
    expect(box.querySelector("iframe, img, script")).toBeNull();
    expect(box.querySelector("figure[data-embed]")).toHaveTextContent("YouTube");
  });

  it("edits an image's alternative text, caption and width", async () => {
    const { box, doc } = await editorOn([{ type: "image", attrs: { media: ID, alt: "old" } }]);
    await userEvent.click(box.querySelector("figure img")!);
    await userEvent.click(screen.getByRole("button", { name: "Image details" }));
    const dialog = await screen.findByRole("dialog", { name: "Image details" });
    const alt = within(dialog).getByLabelText("Alternative text");
    expect(alt).toHaveValue("old");
    await userEvent.clear(alt);
    await userEvent.type(alt, "A cat on a mat");
    await userEvent.type(within(dialog).getByLabelText("Caption"), "Our cat");
    await userEvent.click(within(dialog).getByRole("radio", { name: "Wide" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(doc()?.content[0]).toEqual({ type: "image", attrs: { media: ID, alt: "A cat on a mat", caption: "Our cat", width: "wide" } }));
    const figure = screen.getByRole("textbox", { name: "Body" }).querySelector("figure[data-rich-image]")!;
    expect(figure.getAttribute("data-width")).toBe("wide");
    expect(figure.querySelector("figcaption")).toHaveTextContent("Our cat");
    expect(figure.querySelector("[style]")).toBeNull();
  });

  it("drops pasted images that are not this panel's media, keeps the panel's own", async () => {
    const { box, doc } = await editorOn(null);
    const html = `<p>Hi</p><img src="https://evil.test/x.png" alt="x"><figure data-rich-image data-width="wide"><img src="${SRC}" alt="Ours"><figcaption>Cap</figcaption></figure><img src="data:image/png;base64,AAAA">`;
    fireEvent.paste(box, { clipboardData: { types: ["text/html", "text/plain"], getData: (type: string) => (type === "text/html" ? html : type === "text/plain" ? "Hi" : "") } });
    await waitFor(() => expect(doc()?.content.some((n) => n.type === "image")).toBe(true));
    expect(doc()!.content.filter((n) => n.type === "image")).toEqual([{ type: "image", attrs: { media: ID, alt: "Ours", caption: "Cap", width: "wide" } }]);
    expect(box.querySelectorAll("img")).toHaveLength(1);
    expect(box.querySelector("img")!.getAttribute("src")).toBe(SRC);
  });

  it("wraps paragraphs in callouts and changes their tone", async () => {
    const { doc } = await editorOn(null);
    await userEvent.keyboard("Note");
    await userEvent.click(screen.getByRole("button", { name: "Info callout" }));
    await waitFor(() => expect(doc()?.content[0]).toEqual({ type: "callout", attrs: { tone: "info" }, content: [p("Note")] }));
    expect(screen.getByRole("button", { name: "Info callout" })).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(screen.getByRole("button", { name: "Warning callout" }));
    await waitFor(() => expect(doc()?.content[0]?.attrs).toEqual({ tone: "warn" }));
    await userEvent.click(screen.getByRole("button", { name: "Warning callout" }));
    await waitFor(() => expect(doc()?.content[0]).toEqual(p("Note")));
  });
});

describe("rich text view (version 2)", () => {
  it("shows figures, tables, embed links and callouts as elements", () => {
    const doc = asDocument({ type: "doc", version: 2, content: [
      { type: "image", attrs: { media: ID, alt: "Cat", caption: "<b>Our</b> cat", width: "full" } },
      { type: "table", content: [{ type: "tableRow", content: [cell("tableHeader", "H")] }, { type: "tableRow", content: [cell("tableCell", "D")] }] },
      { type: "embed", attrs: { provider: "vimeo", id: "76979871" } },
      { type: "callout", attrs: { tone: "warn" }, content: [p("Careful")] },
    ] })!;
    const { container } = render(<Providers><RichTextView doc={doc} /></Providers>);
    const figure = container.querySelector("figure.rich-figure")!;
    expect(figure.getAttribute("data-width")).toBe("full");
    expect(figure.querySelector("img")!.getAttribute("alt")).toBe("Cat");
    expect(figure.querySelector("figcaption")!.textContent).toBe("<b>Our</b> cat");
    expect(container.querySelector("thead th")).toHaveTextContent("H");
    expect(container.querySelector("tbody td")).toHaveTextContent("D");
    const link = screen.getByRole("link", { name: "https://vimeo.com/76979871" });
    expect(link).toHaveAttribute("rel", "noopener noreferrer nofollow");
    expect(container.querySelectorAll("img")).toHaveLength(1);
    expect(container.querySelector("iframe")).toBeNull();
    expect(container.querySelector("aside[data-tone=warn]")).toHaveTextContent("Careful");
  });
});
