import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SessionContext } from "@/api/context";
import { createSession } from "@/app/session";
import { type DocNode, asDocument, isEmptyDocument, parseEmbedUrl, toDocument, toEditor } from "@/lib/rich-text";
import { RichEditorWidget, RichTextView } from "@/renderers/rich-text";

// The editor is slow to start in jsdom under load.
configure({ asyncUtilTimeout: 8000 });
vi.setConfig({ testTimeout: 40_000 });

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
        // Lists stay; a heading keeps its text as a paragraph.
        { type: "callout", attrs: { tone: "warn" }, content: [
          { type: "bulletList", content: [{ type: "listItem", content: [p("i")] }] },
          { type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "h" }] },
        ] },
        { type: "embed", attrs: { provider: "x", id: "1460323737035677698" } },
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
      { type: "callout", attrs: { tone: "warn" }, content: [
        { type: "bulletList", content: [{ type: "listItem", content: [p("i")] }] },
        p("h"),
      ] },
    ]);
    // Read back: the editor gets the same attributes.
    const back = toEditor(doc, API);
    expect(back.content?.[1]?.attrs).toEqual({ src: SRC, alt: "b", caption: "Cap", size: "full" });
    expect(toDocument(back, API)).toEqual(doc);
    expect(isEmptyDocument({ type: "doc", version: 2, content: [{ type: "embed", attrs: { provider: "vimeo", id: "1" } }] })).toBe(false);
  });

  it("keep pasted spans in their columns", () => {
    // A spans two rows, B spans two columns: the positions they cover become empty cells.
    const editor: DocNode = { type: "doc", content: [{ type: "table", content: [
      { type: "tableRow", content: [{ type: "tableCell", attrs: { rowspan: 2 }, content: [p("A")] }, { type: "tableCell", attrs: { colspan: 2 }, content: [p("B")] }] },
      { type: "tableRow", content: [cell("tableCell", "C"), cell("tableCell", "D")] },
      { type: "tableRow", content: [cell("tableCell", "E")] },
    ] }] };
    const rows = toDocument(editor, API).content[0]!.content!.map((r) => r.content!.map((c) => c.content![0]!.content![0]?.text ?? ""));
    expect(rows).toEqual([["A", "B", ""], ["", "C", "D"], ["E", "", ""]]);
  });

  it("are edited when the editor can hold them, version 1 as it is", () => {
    const table = { type: "table", content: [{ type: "tableRow", content: [cell("tableHeader", "a")] }, { type: "tableRow", content: [cell("tableCell", "b")] }] };
    const v2 = [table, { type: "embed", attrs: { provider: "vimeo", id: "123" } }, { type: "callout", attrs: { tone: "warn" }, content: [p("w"), { type: "orderedList", content: [{ type: "listItem", content: [p("o")] }] }] },
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
      { type: "callout", attrs: { tone: "info" }, content: [{ type: "bulletList", content: [{ type: "listItem", content: [p("x"), { type: "table", content: [{ type: "tableRow", content: [cell("tableCell", "a")] }] }] }] }] },
      { type: "callout", attrs: { tone: "info" }, content: [{ type: "callout", attrs: { tone: "warn" }, content: [p("x")] }] },
      { type: "embed", attrs: { provider: "x", id: "123" } },
      { type: "embed", attrs: { provider: "vimeo", id: "0123" } },
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
      "https://x.com/rustlang/status/1460323737035677698",
      "https://twitter.com/i/web/status/1460323737035677698",
      "https://vimeo.com/0123",
      "ftp://youtu.be/dQw4w9WgXcQ",
      "not a url",
    ]) expect(parseEmbedUrl(url), url).toBeNull();
  });

  it("read the address as written, refusing what the server refuses (no normalization)", () => {
    for (const url of [
      // A default port, an escape that decodes to a valid id, a tab, a backslash, a dot segment, user info.
      "https://youtu.be:443/dQw4w9WgXcQ",
      "https://www.youtube.com/watch?v=dQw4w9WgXc%51",
      "https://www.youtube.com/watch?v=dQw4w9\tWgXcQ",
      "https://youtu.be\\dQw4w9WgXcQ",
      "https://youtu.be/x/../dQw4w9WgXcQ",
      "https://youtu.be/./dQw4w9WgXcQ",
      "https://user@youtu.be/dQw4w9WgXcQ",
      "https://youtu.be:/dQw4w9WgXcQ",
    ]) expect(parseEmbedUrl(url), url).toBeNull();
    // A leading U+FEFF is not trimmed (not Unicode White_Space, as for the server).
    expect(parseEmbedUrl("\uFEFFhttps://youtu.be/dQw4w9WgXcQ")).toBeNull();
    // Case-insensitive scheme and host, as the server reads them.
    expect(parseEmbedUrl("HTTPS://YOUTU.BE/dQw4w9WgXcQ#t=1")).toEqual({ provider: "youtube", id: "dQw4w9WgXcQ" });
    // What the server does not read is not checked: escapes elsewhere in the query, a Vimeo hash, a fragment.
    for (const [url, embed] of [
      ["https://youtu.be/dQw4w9WgXcQ?next=%2F", { provider: "youtube", id: "dQw4w9WgXcQ" }],
      ["https://www.youtube.com/watch?x=%41&v=dQw4w9WgXcQ", { provider: "youtube", id: "dQw4w9WgXcQ" }],
      ["https://vimeo.com/76979871/%41b", { provider: "vimeo", id: "76979871" }],
      ["https://youtu.be/dQw4w9WgXcQ#a\\b%", { provider: "youtube", id: "dQw4w9WgXcQ" }],
      ["\u00a0https://youtu.be/dQw4w9WgXcQ\n", { provider: "youtube", id: "dQw4w9WgXcQ" }],
    ] as const) expect(parseEmbedUrl(url), url).toEqual(embed);
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
    // Without the table, nothing needs version 2 any more.
    expect(doc()!.version).toBe(1);
    expect(screen.queryByRole("toolbar", { name: "Table" })).toBeNull();
  });

  it("does not grow a table past 50 rows or 20 columns", async () => {
    const row = (n: number) => ({ type: "tableRow", content: Array.from({ length: n }, () => cell("tableCell", "x")) });
    const tools = async (content: DocNode[]) => {
      const { box } = await editorOn(content);
      // Clicking a cell puts the cursor in the table (retried, as for images).
      return waitFor(async () => {
        await userEvent.click(box.querySelector("td p")!);
        return screen.getByRole("toolbar", { name: "Table" });
      });
    };
    let bar = await tools([{ type: "table", content: Array.from({ length: 50 }, () => row(1)) }]);
    expect(within(bar).getByRole("button", { name: "Add row" })).toBeDisabled();
    expect(within(bar).getByRole("button", { name: "Add column" })).toBeEnabled();
    expect(within(bar).getByRole("button", { name: "Remove column" })).toBeDisabled();
    cleanup();
    bar = await tools([{ type: "table", content: [row(20)] }]);
    expect(within(bar).getByRole("button", { name: "Add column" })).toBeDisabled();
    expect(within(bar).getByRole("button", { name: "Add row" })).toBeEnabled();
    expect(within(bar).getByRole("button", { name: "Remove row" })).toBeDisabled();
  });

  it("never adds rows past the limit with Tab, and toggles the whole header row", async () => {
    const row = (type: string, n: number) => ({ type: "tableRow", content: Array.from({ length: n }, () => cell(type, "x")) });
    const { box, doc } = await editorOn([{ type: "table", content: Array.from({ length: 50 }, () => row("tableCell", 1)) }]);
    const last = () => [...box.querySelectorAll("td p")].at(-1)!;
    await waitFor(async () => {
      await userEvent.click(last());
      screen.getByRole("toolbar", { name: "Table" });
    });
    await userEvent.keyboard("{Tab}");
    await new Promise((r) => setTimeout(r, 50));
    expect(box.querySelectorAll("tr")).toHaveLength(50);
    expect(doc()?.content[0]?.content).toHaveLength(50);
    cleanup();
    // A header row of several cells: toggled as a whole.
    const second = await editorOn([{ type: "table", content: [row("tableHeader", 3)] }]);
    await waitFor(async () => {
      await userEvent.click(second.box.querySelector("th p")!);
      screen.getByRole("toolbar", { name: "Table" });
    });
    const header = within(screen.getByRole("toolbar", { name: "Table" })).getByRole("button", { name: "Header row" });
    expect(header).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(header);
    await waitFor(() => expect(second.doc()?.content[0]?.content?.[0]?.content?.map((c) => c.type)).toEqual(["tableCell", "tableCell", "tableCell"]));
    expect(second.box.querySelector("th")).toBeNull();
    expect(header).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(header);
    await waitFor(() => expect(second.doc()?.content[0]?.content?.[0]?.content?.map((c) => c.type)).toEqual(["tableHeader", "tableHeader", "tableHeader"]));
  });

  it("embeds an allowed address as {provider, id}, shown as a card without iframe or image", async () => {
    const { box, doc } = await editorOn(null);
    await userEvent.click(screen.getByRole("button", { name: "Embed a video" }));
    const dialog = await screen.findByRole("dialog", { name: "Embed a video" });
    const address = within(dialog).getByLabelText("Address");
    await userEvent.type(address, "https://evil.test/watch?v=dQw4w9WgXcQ{Enter}");
    expect(within(dialog).getByRole("alert")).toHaveTextContent("Only YouTube and Vimeo");
    await userEvent.clear(address);
    await userEvent.type(address, "https://youtu.be/dQw4w9WgXcQ?t=1");
    expect(within(dialog).getByText("https://www.youtube.com/watch?v=dQw4w9WgXcQ")).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(doc()?.content.find((n) => n.type === "embed")).toEqual({ type: "embed", attrs: { provider: "youtube", id: "dQw4w9WgXcQ" } }));
    expect(box.querySelector("iframe, img, script")).toBeNull();
    expect(box.querySelector("figure[data-embed]")).toHaveTextContent("YouTube · dQw4w9WgXcQ");
  });

  it("edits an image's alternative text, caption and width", async () => {
    const { box, doc } = await editorOn([{ type: "image", attrs: { media: ID, alt: "old" } }]);
    const details = screen.getByRole("button", { name: "Image details" });
    // Clicking the image selects it (retried: under load the first click can land before the view is ready).
    await waitFor(async () => {
      await userEvent.click(box.querySelector("figure img")!);
      expect(details).toBeEnabled();
    });
    await userEvent.click(details);
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

  it("reads a pasted figure's image from its checked parts only, never the figure's own attributes", async () => {
    const { box, doc } = await editorOn(null);
    // The figure's `src`, `alt`, `caption` and `size` attributes are not ours to read.
    const html = `<p>Hi</p><figure data-rich-image src="https://evil.test/x.png" alt="evil" caption="evil" size="full" data-width="huge"><img src="${SRC}"></figure>` +
      `<figure data-rich-image src="${SRC}"><img src="https://evil.test/y.png"></figure>`;
    fireEvent.paste(box, { clipboardData: { types: ["text/html", "text/plain"], getData: (type: string) => (type === "text/html" ? html : type === "text/plain" ? "Hi" : "") } });
    await waitFor(() => expect(doc()?.content.some((n) => n.type === "image")).toBe(true));
    // The second figure's image is not ours: it is dropped whatever the figure says.
    expect(doc()!.content.filter((n) => n.type === "image")).toEqual([{ type: "image", attrs: { media: ID, alt: "" } }]);
    expect(box.querySelectorAll("img")).toHaveLength(1);
    expect(box.querySelector("img")!.getAttribute("src")).toBe(SRC);
    expect(box.querySelector("figure")!.getAttribute("data-width")).toBe("normal");
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
    // A list goes into a callout too; no table can be inserted there.
    await userEvent.click(screen.getByRole("button", { name: "Bulleted list" }));
    await userEvent.click(screen.getByRole("button", { name: "Info callout" }));
    await waitFor(() => expect(doc()?.content[0]).toEqual({ type: "callout", attrs: { tone: "info" }, content: [
      { type: "bulletList", content: [{ type: "listItem", content: [p("Note")] }] },
    ] }));
    expect(screen.getByRole("button", { name: "Insert table" })).toBeDisabled();
    // Unwrapped as a whole: the list stays a list.
    await userEvent.click(screen.getByRole("button", { name: "Info callout" }));
    await waitFor(() => expect(doc()?.content[0]).toEqual({ type: "bulletList", content: [{ type: "listItem", content: [p("Note")] }] }));
  });

  it("offers no rule, table or embed inside a table cell or a callout", async () => {
    const callout = { type: "callout", attrs: { tone: "info" }, content: [p("note")] };
    const table = { type: "table", content: [{ type: "tableRow", content: [cell("tableCell", "x")] }] };
    const { box } = await editorOn([p("top"), callout, table]);
    const off = () => ["Horizontal rule", "Insert table", "Embed a video"].map((name) => screen.getByRole("button", { name }).hasAttribute("disabled"));
    await waitFor(async () => {
      await userEvent.click(box.querySelector("p")!);
      expect(off()).toEqual([false, false, false]);
    });
    await waitFor(async () => {
      await userEvent.click(box.querySelector("aside p")!);
      expect(off()).toEqual([true, true, true]);
    });
    await waitFor(async () => {
      await userEvent.click(box.querySelector("td p")!);
      expect(off()).toEqual([true, true, true]);
    });
  });

  it("unwraps a callout keeping the selection and the other callouts", async () => {
    const callout = (text: string) => ({ type: "callout", attrs: { tone: "info" }, content: [p(text)] });
    const { box, doc } = await editorOn([callout("abc"), callout("def")]);
    await waitFor(async () => {
      await userEvent.click(box.querySelector("aside p")!);
      expect(screen.getByRole("button", { name: "Info callout" })).toHaveAttribute("aria-pressed", "true");
    });
    await userEvent.click(screen.getByRole("button", { name: "Info callout" }));
    await waitFor(() => expect(doc()?.content.slice(0, 2)).toEqual([p("abc"), callout("def")]));
    // The cursor is still in "abc": typing goes there.
    await userEvent.keyboard("!");
    await waitFor(() => expect(JSON.stringify(doc()?.content[0])).toContain("!"));
    expect(doc()?.content[1]).toEqual(callout("def"));
  });

  it("never wraps a list holding a table in a callout, and never drops content", async () => {
    const list = { type: "bulletList", content: [{ type: "listItem", content: [p("item"), { type: "table", content: [{ type: "tableRow", content: [cell("tableCell", "kept")] }] }] }] };
    const { box, doc } = await editorOn([list]);
    await waitFor(async () => {
      await userEvent.click(box.querySelector("li > p")!);
      expect(box.querySelector("li > p")).toBeTruthy();
    });
    await userEvent.click(screen.getByRole("button", { name: "Info callout" }));
    await userEvent.keyboard("!");
    await waitFor(() => expect(doc()).not.toBeNull());
    expect(doc()!.content.some((n) => n.type === "callout")).toBe(false);
    expect(JSON.stringify(doc())).toContain("kept");
    // A pasted callout holding a table keeps it: the server refuses it with a message instead.
    const pasted = toDocument({ type: "doc", content: [{ type: "callout", attrs: { tone: "info" }, content: [list] }] }, API);
    expect(JSON.stringify(pasted)).toContain("kept");
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
