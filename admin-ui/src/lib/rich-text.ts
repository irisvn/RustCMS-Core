// Rich text documents (see `rustcms_admin::rich_text`): the stored JSON, and the
// editor's (TipTap) JSON. Only the nodes, marks and attributes the server
// accepts survive the mapping; images travel as media ids, shown through the
// panel's own media URL; embeds as a provider and an id (never a URL or HTML).
// Version 1 documents are read as they are (a subset of version 2); the
// editor saves version 2.

export interface DocNode {
  type: string;
  attrs?: Record<string, unknown>;
  content?: DocNode[];
  text?: string;
  marks?: { type: string; attrs?: Record<string, unknown> }[];
}

export interface RichDocument {
  type: "doc";
  version: 1 | 2;
  content: DocNode[];
}

/** The version the editor saves. */
export const DOCUMENT_VERSION = 2;

/** The server's limits (`rustcms_admin::rich_text`). */
export const LIMITS = { alt: 500, caption: 1000, tableRows: 50, tableColumns: 20 } as const;

/** How wide an image is shown (a closed set). */
export const IMAGE_WIDTHS = ["normal", "wide", "full"] as const;
export type ImageWidth = (typeof IMAGE_WIDTHS)[number];

/** Callout tones. */
export const CALLOUT_TONES = ["info", "warn"] as const;
export type CalloutTone = (typeof CALLOUT_TONES)[number];

const BLOCKS = new Set(["paragraph", "heading", "bulletList", "orderedList", "listItem", "blockquote", "codeBlock", "horizontalRule", "image", "table", "embed", "callout"]);
const INLINES = new Set(["text", "hardBreak"]);
const MARKS = new Set(["bold", "italic", "strike", "code", "link"]);
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** `http(s)://` or `mailto:` without spaces or controls, at most 2,048 bytes (the server's rule). */
export function safeHref(href: string): boolean {
  const lower = href.toLowerCase();
  const bytes = new TextEncoder().encode(href).length;
  // eslint-disable-next-line no-control-regex
  return bytes <= 2048 && href.length > 7 && !/[\s\u0000-\u001f\u007f-\u009f]/.test(href) &&
    (lower.startsWith("https://") || lower.startsWith("http://") || lower.startsWith("mailto:"));
}

/** The panel URL of a media item's file. */
export const mediaUrl = (apiPath: string, id: string) => `${apiPath}/media/${encodeURIComponent(id)}/file`;

/** The media id behind one of the panel's media URLs, or null. */
export function mediaIdOf(apiPath: string, src: unknown): string | null {
  if (typeof src !== "string") return null;
  const prefix = `${apiPath}/media/`;
  if (!src.startsWith(prefix) || !src.endsWith("/file")) return null;
  const id = decodeURIComponent(src.slice(prefix.length, -"/file".length));
  return UUID.test(id) ? id : null;
}

// --- Embeds ---------------------------------------------------------------

export type EmbedProvider = "youtube" | "vimeo";

export interface Embed {
  provider: EmbedProvider;
  id: string;
}

const EMBED_IDS: Record<EmbedProvider, RegExp> = {
  youtube: /^[A-Za-z0-9_-]{11}$/,
  vimeo: /^[1-9][0-9]{0,14}$/,
};

/** Whether `provider` and `id` are an allowed embed (the server's rule). */
export function validEmbed(provider: unknown, id: unknown): provider is EmbedProvider {
  return typeof provider === "string" && typeof id === "string" && Object.hasOwn(EMBED_IDS, provider) &&
    EMBED_IDS[provider as EmbedProvider].test(id);
}

/** The provider's public page for an embed (as `to_html` links it). */
export function embedUrl(embed: Embed): string {
  switch (embed.provider) {
    case "youtube":
      return `https://www.youtube.com/watch?v=${embed.id}`;
    case "vimeo":
      return `https://vimeo.com/${embed.id}`;
  }
}

/** Provider names as shown. */
export const EMBED_NAMES: Record<EmbedProvider, string> = { youtube: "YouTube", vimeo: "Vimeo" };

/** Unicode `White_Space` (Rust's `char::is_whitespace`; unlike `\s`, not U+FEFF), trimmed as `str::trim` does. */
const WHITE = "\\u0009-\\u000d\\u0020\\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000";
/** `White_Space` or a control (`char::is_control`). */
const SPACE_OR_CONTROL = new RegExp(`[${WHITE}\\u0000-\\u001f\\u007f-\\u009f]`);
const EDGES = new RegExp(`^[${WHITE}]+|[${WHITE}]+$`, "g");

/**
 * The embed a pasted address points to, from an allowlist of hosts and
 * paths (YouTube, Vimeo), or null. Only the provider and the id are kept.
 * A port of the server's rule (`rustcms_admin::rich_text::Embed::from_url`),
 * part by part, so both accept the same addresses: the address is read as
 * written, never normalized or decoded (`new URL` would drop a default port,
 * decode escapes, turn a backslash into `/` and resolve `..`). An escape, a
 * backslash or a dot segment is refused where it is read (the authority, the
 * path parts, the id) by the host and id checks, and ignored elsewhere.
 */
export function parseEmbedUrl(input: string): Embed | null {
  const raw = input.replace(EDGES, "");
  const scheme = /^https?:\/\//i.exec(raw);
  if (!scheme || SPACE_OR_CONTROL.test(raw)) return null;
  const rest = raw.slice(scheme[0].length);
  const end = rest.search(/[/?#]/);
  const authority = end === -1 ? rest : rest.slice(0, end);
  // No user info, no port (not even the default one).
  if (/[@:]/.test(authority)) return null;
  const host = authority.replace(/[A-Z]/g, (c) => c.toLowerCase()).replace(/^(www|m|mobile)\./, "");
  const target = rest.slice(authority.length).split("#")[0]!;
  const at = target.indexOf("?");
  const path = at === -1 ? target : target.slice(0, at);
  const query = at === -1 ? "" : target.slice(at + 1);
  const parts = path.split("/").filter(Boolean);
  const v = query.split("&").find((pair) => pair.startsWith("v="))?.slice(2);
  const pick = (provider: EmbedProvider, id: string | null | undefined): Embed | null =>
    id && validEmbed(provider, id) ? { provider, id } : null;
  switch (host) {
    case "youtube.com":
    case "youtube-nocookie.com":
      if (parts[0] === "watch" && parts.length === 1) return pick("youtube", v);
      if (["embed", "shorts", "live", "v"].includes(parts[0] ?? "") && parts.length === 2) return pick("youtube", parts[1]);
      return null;
    case "youtu.be":
      return parts.length === 1 ? pick("youtube", parts[0]) : null;
    case "vimeo.com":
      // `vimeo.com/123` and `vimeo.com/123/hash` (unlisted: the hash is not kept).
      return parts.length >= 1 && parts.length <= 2 ? pick("vimeo", parts[0]) : null;
    case "player.vimeo.com":
      return parts[0] === "video" && parts.length === 2 ? pick("vimeo", parts[1]) : null;
    default:
      return null;
  }
}

// --- Checking and mapping -------------------------------------------------

/** Paragraphs only, at least one, each holding inline content (table cells, callouts). */
function paragraphs(nodes: unknown): boolean {
  return Array.isArray(nodes) && nodes.length > 0 &&
    nodes.every((p: DocNode) => !!p && p.type === "paragraph" && !p.attrs && editable(p.content, "inline"));
}

/** Whether any node at any depth is of one of `types`. */
function holds(nodes: unknown, types: string[]): boolean {
  return Array.isArray(nodes) && nodes.some((n: DocNode) => !!n && (types.includes(n.type) || holds(n.content, types)));
}

/** A table the editor can hold as it is: rectangular rows of cells, header cells in the first row only. */
function editableTable(n: DocNode): boolean {
  const rows = n.content;
  if (n.attrs || !Array.isArray(rows) || rows.length === 0) return false;
  const header = rows[0]?.content?.[0]?.type === "tableHeader";
  const width = rows[0]?.content?.length ?? 0;
  return width > 0 && rows.every((row, r) =>
    row?.type === "tableRow" && !row.attrs && Array.isArray(row.content) && row.content.length === width &&
    row.content.every((cell) => cell?.type === (header && r === 0 ? "tableHeader" : "tableCell") && !cell.attrs && paragraphs(cell.content)));
}

const CONTAINERS: Record<string, "inline" | "blocks" | "items" | "text" | "none"> = {
  paragraph: "inline", heading: "inline", bulletList: "items", orderedList: "items", listItem: "blocks",
  blockquote: "blocks", codeBlock: "text", horizontalRule: "none", image: "none",
};

/** Whether every node is one the editor holds where it holds it (else editing would drop content). */
function editable(nodes: unknown, kind: "blocks" | "inline" | "items" | "text", version: number = DOCUMENT_VERSION): boolean {
  if (nodes === undefined) return true;
  if (!Array.isArray(nodes)) return false;
  return nodes.every((raw: unknown) => {
    if (!raw || typeof raw !== "object") return false;
    const n = raw as DocNode;
    if (kind === "text" || kind === "inline") {
      if (n.type === "hardBreak") return kind === "inline";
      if (n.type !== "text" || typeof n.text !== "string") return false;
      return kind === "inline" ? (n.marks ?? []).every((m) => MARKS.has(m.type)) : !n.marks;
    }
    if (kind === "items") return n.type === "listItem" && editable(n.content, "blocks", version);
    if (n.type === "listItem") return false;
    // Version 2 nodes.
    if (n.type === "table") return version >= 2 && editableTable(n);
    if (n.type === "embed") return version >= 2 && n.content === undefined && validEmbed(n.attrs?.provider, n.attrs?.id);
    if (n.type === "callout") {
      return version >= 2 && CALLOUT_TONES.includes(n.attrs?.tone as CalloutTone) && Array.isArray(n.content) && n.content.length > 0 &&
        n.content.every((c) => ["paragraph", "bulletList", "orderedList"].includes(c?.type)) && !holds(n.content, ["table", "callout"]) &&
        editable(n.content, "blocks", version);
    }
    if (!(n.type in CONTAINERS)) return false;
    if (n.type === "image") {
      const a = n.attrs ?? {};
      const fits = (a.caption === undefined && a.width === undefined) || version >= 2;
      return fits && typeof a.media === "string" && UUID.test(a.media) &&
        (a.width === undefined || IMAGE_WIDTHS.includes(a.width as ImageWidth));
    }
    const inner = CONTAINERS[n.type] ?? "none";
    return inner === "none" ? n.content === undefined : editable(n.content, inner, version);
  });
}

/**
 * A stored document the editor can hold without losing anything, or null (the
 * value is then shown read-only and never rewritten). Versions 1 and 2.
 */
export function asDocument(value: unknown): RichDocument | null {
  if (!value || typeof value !== "object") return null;
  const doc = value as RichDocument;
  return doc.type === "doc" && (doc.version === 1 || doc.version === 2) && editable(doc.content, "blocks", doc.version) ? doc : null;
}

/** Cut to `max` code points, as the server counts. */
const cut = (text: unknown, max: number) => (typeof text === "string" ? [...text].slice(0, max).join("") : "");

/**
 * Editor rows → stored rows: rectangular (a spanned cell keeps its content
 * in its first position, the other positions it covers, across columns and
 * rows, become empty cells; short rows are padded), header cells in the first
 * row only, and only when that row is all header cells.
 */
function tableRows(rows: DocNode[], cell: (c: DocNode) => DocNode[]): DocNode[] {
  const first = rows[0]?.content ?? [];
  const header = first.length > 0 && first.every((c) => c.type === "tableHeader");
  const empty = (): DocNode[] => [{ type: "paragraph", content: [] }];
  const span = (v: unknown) => Math.max(1, Math.min(Number(v) || 1, LIMITS.tableColumns + LIMITS.tableRows));
  // Positions covered by a cell of an earlier row (rowspan).
  const grid: (DocNode[] | null)[][] = rows.map(() => []);
  rows.forEach((row, r) => {
    let column = 0;
    for (const c of row.content ?? []) {
      while (grid[r]![column] !== undefined) column += 1;
      const width = span(c.attrs?.colspan);
      const height = Math.min(span(c.attrs?.rowspan), rows.length - r);
      for (let dr = 0; dr < height; dr++) {
        for (let dc = 0; dc < width; dc++) grid[r + dr]![column + dc] = dr === 0 && dc === 0 ? cell(c) : null;
      }
      column += width;
    }
  });
  const width = Math.max(1, ...grid.map((r) => r.length));
  return grid.map((cells, r) => {
    const type = header && r === 0 ? "tableHeader" : "tableCell";
    const content = Array.from({ length: width }, (_, i) => {
      const at = cells[i];
      return { type, content: at && at.length ? at : empty() };
    });
    return { type: "tableRow", content };
  });
}

/** Editor JSON → stored document (version 2): unknown parts dropped, images as media ids. */
export function toDocument(editor: DocNode, apiPath: string): RichDocument {
  const node = (n: DocNode, inline: boolean): DocNode | null => {
    if (!(inline ? INLINES : BLOCKS).has(n.type)) return null;
    switch (n.type) {
      case "text": {
        if (!n.text) return null;
        const marks = (n.marks ?? []).flatMap((m) => {
          if (!MARKS.has(m.type)) return [];
          if (m.type !== "link") return [{ type: m.type }];
          const href = String(m.attrs?.href ?? "");
          return safeHref(href) ? [{ type: "link", attrs: { href } }] : [];
        });
        return marks.length ? { type: "text", text: n.text, marks } : { type: "text", text: n.text };
      }
      case "image": {
        const id = mediaIdOf(apiPath, n.attrs?.src);
        if (!id) return null;
        const attrs: Record<string, unknown> = { media: id, alt: cut(n.attrs?.alt, LIMITS.alt) };
        const caption = cut(n.attrs?.caption, LIMITS.caption);
        if (caption) attrs.caption = caption;
        const width = n.attrs?.size;
        if (width === "wide" || width === "full") attrs.width = width;
        return { type: "image", attrs };
      }
      case "heading": {
        const level = Number(n.attrs?.level);
        return { type: "heading", attrs: { level: level >= 2 && level <= 4 ? level : 2 }, content: children(n, true) };
      }
      case "codeBlock": {
        const text = (n.content ?? []).map((c) => c.text ?? "").join("");
        return text ? { type: "codeBlock", content: [{ type: "text", text }] } : { type: "codeBlock" };
      }
      case "horizontalRule":
      case "hardBreak":
        return { type: n.type };
      case "paragraph":
        return { type: "paragraph", content: children(n, true) };
      case "embed": {
        const { provider, id } = n.attrs ?? {};
        return validEmbed(provider, id) ? { type: "embed", attrs: { provider, id } } : null;
      }
      case "callout": {
        const tone = n.attrs?.tone === "warn" ? "warn" : "info";
        // Paragraphs and lists; another block keeps its text as a paragraph. A table or callout
        // inside a list is kept (never dropped silently): the server refuses it with a message.
        const content = (n.content ?? []).map((c) => (["paragraph", "bulletList", "orderedList"].includes(c.type) ? node(c, false) : node({ type: "paragraph", content: c.content }, false)))
          .filter((c): c is DocNode => c !== null);
        return { type: "callout", attrs: { tone }, content: content.length ? content : [{ type: "paragraph", content: [] }] };
      }
      case "table": {
        const rows = (n.content ?? []).filter((r) => r.type === "tableRow");
        return rows.length ? { type: "table", content: tableRows(rows, (c) => paras(c.content ?? [])) } : null;
      }
      default:
        return { type: n.type, content: children(n, false) };
    }
  };
  // Paragraphs only (cells): another block keeps its text as a paragraph.
  const paras = (list: DocNode[]): DocNode[] =>
    list.map((c) => (c.type === "paragraph" ? node(c, false) : { type: "paragraph", content: children(c, true) }))
      .filter((c): c is DocNode => c !== null);
  const children = (n: DocNode, inline: boolean) =>
    (n.content ?? []).map((c) => node(c, inline)).filter((c): c is DocNode => c !== null);
  const content = children(editor, false);
  return { type: "doc", version: needsV2(content) ? 2 : 1, content };
}

/**
 * Whether content uses what only version 2 has (tables, embeds, callouts,
 * image captions and widths). Documents without it are written as version 1,
 * which 0.9 still reads (a rollback, an older reader of the same database).
 */
function needsV2(content: DocNode[]): boolean {
  return content.some((n) =>
    ["table", "embed", "callout"].includes(n.type) ||
    (n.type === "image" && (n.attrs?.caption !== undefined || n.attrs?.width !== undefined)) ||
    needsV2(n.content ?? []));
}

/** Stored document → editor JSON (images shown from the panel's media URL). */
export function toEditor(doc: RichDocument, apiPath: string): DocNode {
  const node = (n: DocNode): DocNode => {
    if (n.type === "image") {
      const a = n.attrs ?? {};
      return { type: "image", attrs: { src: mediaUrl(apiPath, String(a.media)), alt: a.alt ?? "", caption: a.caption ?? "", size: a.width ?? "normal" } };
    }
    return { ...n, content: n.content?.map(node) };
  };
  return { type: "doc", content: doc.content.length ? doc.content.map(node) : [{ type: "paragraph" }] };
}

/** Whether a document has no text, image, embed or table (saved as "not set" when allowed). */
export function isEmptyDocument(doc: RichDocument): boolean {
  const has = (n: DocNode): boolean =>
    ["image", "horizontalRule", "embed", "table"].includes(n.type) || !!n.text || (n.content ?? []).some(has);
  return !doc.content.some(has);
}
