// Rich text documents (see `rustcms_admin::rich_text`): the stored JSON, and the
// editor's (TipTap) JSON. Only the nodes, marks and attributes the server
// accepts survive the mapping; images travel as media ids, shown through the
// panel's own media URL.

export interface DocNode {
  type: string;
  attrs?: Record<string, unknown>;
  content?: DocNode[];
  text?: string;
  marks?: { type: string; attrs?: Record<string, unknown> }[];
}

export interface RichDocument {
  type: "doc";
  version: 1;
  content: DocNode[];
}

const BLOCKS = new Set(["paragraph", "heading", "bulletList", "orderedList", "listItem", "blockquote", "codeBlock", "horizontalRule", "image"]);
const INLINES = new Set(["text", "hardBreak", "image"]);
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

const CONTAINERS: Record<string, "inline" | "blocks" | "items" | "text" | "none"> = {
  paragraph: "inline", heading: "inline", bulletList: "items", orderedList: "items", listItem: "blocks",
  blockquote: "blocks", codeBlock: "text", horizontalRule: "none", image: "none",
};

/** Whether every node is one the editor holds where it holds it (else editing would drop content). */
function editable(nodes: unknown, kind: "blocks" | "inline" | "items" | "text"): boolean {
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
    if (kind === "items" ? n.type !== "listItem" : n.type === "listItem" || !(n.type in CONTAINERS)) return false;
    if (n.type === "image") return typeof n.attrs?.media === "string" && UUID.test(n.attrs.media);
    const inner = CONTAINERS[n.type] ?? "none";
    return inner === "none" ? n.content === undefined : editable(n.content, inner);
  });
}

/**
 * A stored document the editor can hold without losing anything, or null (the
 * value is then shown read-only and never rewritten).
 */
export function asDocument(value: unknown): RichDocument | null {
  if (!value || typeof value !== "object") return null;
  const doc = value as RichDocument;
  return doc.type === "doc" && doc.version === 1 && editable(doc.content, "blocks") ? doc : null;
}

/** Editor JSON → stored document: unknown parts dropped, images as media ids. */
export function toDocument(editor: DocNode, apiPath: string): RichDocument {
  const node = (n: DocNode, inline: boolean): DocNode | null => {
    if (!(inline ? INLINES : BLOCKS).has(n.type) && n.type !== "listItem") return null;
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
        // Code points, as the server counts.
        const alt = typeof n.attrs?.alt === "string" ? [...n.attrs.alt].slice(0, 500).join("") : "";
        return { type: "image", attrs: { media: id, alt } };
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
      default:
        return { type: n.type, content: children(n, false) };
    }
  };
  const children = (n: DocNode, inline: boolean) =>
    (n.content ?? []).map((c) => node(c, inline)).filter((c): c is DocNode => c !== null);
  return { type: "doc", version: 1, content: children(editor, false) };
}

/** Stored document → editor JSON (images shown from the panel's media URL). */
export function toEditor(doc: RichDocument, apiPath: string): DocNode {
  const node = (n: DocNode): DocNode =>
    n.type === "image"
      ? { type: "image", attrs: { src: mediaUrl(apiPath, String(n.attrs?.media)), alt: n.attrs?.alt ?? "" } }
      : { ...n, content: n.content?.map(node) };
  return { type: "doc", content: doc.content.length ? doc.content.map(node) : [{ type: "paragraph" }] };
}

/** Whether a document has no text and no image (saved as "not set" when allowed). */
export function isEmptyDocument(doc: RichDocument): boolean {
  const has = (n: DocNode): boolean => n.type === "image" || n.type === "horizontalRule" || !!n.text || (n.content ?? []).some(has);
  return !doc.content.some(has);
}
