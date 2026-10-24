// The editor's own nodes (TipTap): images with a caption and a width, tables
// (paragraphs in cells, no spans, no widths), embeds (a provider and an id,
// shown as a card: no iframe, no third-party image) and callouts. Nothing
// here renders a `style` attribute or inline script: the CSP allows neither.
import { Node, findParentNode, mergeAttributes } from "@tiptap/core";
import type { EditorState } from "@tiptap/pm/state";
import { findWrapping, liftTarget } from "@tiptap/pm/transform";
import { TableMap } from "@tiptap/pm/tables";
import Image from "@tiptap/extension-image";
import { Table, TableCell, TableHeader, TableRow } from "@tiptap/extension-table";
import { CALLOUT_TONES, EMBED_NAMES, type CalloutTone, IMAGE_WIDTHS, type ImageWidth, LIMITS, embedUrl, mediaIdOf, validEmbed } from "@/lib/rich-text";

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    embed: {
      /** Inserts an embed (checked: an allowed provider and id). */
      setEmbed: (attrs: { provider: string; id: string }) => ReturnType;
    };
    callout: {
      /** Wraps the selected paragraphs in a callout of `tone`, changes its tone, or (same tone) unwraps it. */
      toggleCallout: (tone: CalloutTone) => ReturnType;
    };
  }
}

/**
 * Images from the media library: a figure with the image and its caption.
 * Pasted or dropped HTML keeps only images of this panel's media URLs (others
 * are dropped as they arrive, as the server would refuse them).
 */
export const RichImage = Image.extend<{ apiPath: string; inline: boolean; allowBase64: boolean; HTMLAttributes: Record<string, unknown>; resize: false }>({
  addOptions() {
    return { ...this.parent!(), apiPath: "", resize: false as const };
  },
  addAttributes() {
    return {
      src: { default: null },
      alt: { default: "" },
      caption: { default: "" },
      size: { default: "normal" },
    };
  },
  parseHTML() {
    const ours = (src: string | null) => mediaIdOf(this.options.apiPath, src) !== null;
    return [
      {
        tag: "figure[data-rich-image]",
        getAttrs: (el) => {
          const img = el.querySelector("img");
          const src = img?.getAttribute("src") ?? null;
          if (!ours(src)) return false;
          const size = el.getAttribute("data-width");
          return {
            src,
            alt: img?.getAttribute("alt") ?? "",
            caption: el.querySelector("figcaption")?.textContent ?? "",
            size: IMAGE_WIDTHS.includes(size as ImageWidth) ? size : "normal",
          };
        },
      },
      { tag: "img[src]", getAttrs: (el) => (ours(el.getAttribute("src")) ? { src: el.getAttribute("src"), alt: el.getAttribute("alt") ?? "" } : false) },
    ];
  },
  renderHTML({ node }) {
    const img = ["img", { src: node.attrs.src, alt: node.attrs.alt ?? "" }] as const;
    const figure = { "data-rich-image": "", "data-width": node.attrs.size, class: "rich-figure" };
    return node.attrs.caption ? ["figure", figure, img, ["figcaption", {}, String(node.attrs.caption)]] : ["figure", figure, img];
  },
  // No Markdown shortcut (`![](https://…)`) and no resizing (inline sizes).
  addInputRules() {
    return [];
  },
  addNodeView() {
    return null;
  },
});

/** Whether the table at the selection may grow by `rows` and `columns` within the server's limits. */
function roomFor(state: EditorState, rows: number, columns: number): boolean {
  const table = findParentNode((n) => n.type.name === "table")(state.selection);
  if (!table) return true;
  const map = TableMap.get(table.node);
  return map.height + rows <= LIMITS.tableRows && map.width + columns <= LIMITS.tableColumns;
}

/**
 * Tables without inline styles: no column widths, no resizing, no alignment.
 * Rows and columns are added within the server's limits on every path (the
 * toolbar, Tab in the last cell), and the header is the whole first row.
 */
export const RichTable = Table.extend({
  addOptions() {
    return { ...this.parent!(), resizable: false, View: null, allowTableNodeSelection: false };
  },
  addCommands() {
    const parent = this.parent!();
    return {
      ...parent,
      addRowBefore: () => (props) => roomFor(props.state, 1, 0) && parent.addRowBefore!()(props),
      addRowAfter: () => (props) => roomFor(props.state, 1, 0) && parent.addRowAfter!()(props),
      addColumnBefore: () => (props) => roomFor(props.state, 0, 1) && parent.addColumnBefore!()(props),
      addColumnAfter: () => (props) => roomFor(props.state, 0, 1) && parent.addColumnAfter!()(props),
      // The whole first row becomes header cells, or ordinary cells (no header columns in documents).
      toggleHeaderRow: () => ({ state, tr, dispatch }) => {
        const table = findParentNode((n) => n.type.name === "table")(state.selection);
        const first = table?.node.firstChild;
        if (!table || !first) return false;
        const header = first.childCount > 0 && [...Array(first.childCount).keys()].every((i) => first.child(i).type.name === "tableHeader");
        const type = state.schema.nodes[header ? "tableCell" : "tableHeader"]!;
        if (dispatch) first.forEach((cell, offset) => tr.setNodeMarkup(table.pos + 2 + offset, type, cell.attrs));
        return true;
      },
    };
  },
  renderHTML({ HTMLAttributes }) {
    return ["table", mergeAttributes(HTMLAttributes, { class: "rich-table" }), ["tbody", 0]];
  },
  addNodeView() {
    return null;
  },
});

const cellAttributes = {
  colspan: { default: 1 },
  rowspan: { default: 1 },
  colwidth: { default: null, rendered: false },
};

/** Cells hold paragraphs (inline marks) only, as the server requires. */
export const RichTableCell = TableCell.extend({
  content: "paragraph+",
  addAttributes: () => cellAttributes,
});

export const RichTableHeader = TableHeader.extend({
  content: "paragraph+",
  addAttributes: () => cellAttributes,
});

export const RichTableRow = TableRow;

/** A video: a provider and an id, shown as a text card (provider, id, page): no iframe, no thumbnail. */
export const RichEmbed = Node.create({
  name: "embed",
  group: "block",
  atom: true,
  draggable: true,
  selectable: true,
  addAttributes() {
    return { provider: { default: null }, id: { default: null } };
  },
  parseHTML() {
    return [{
      tag: "figure[data-embed]",
      getAttrs: (el) => {
        const provider = el.getAttribute("data-provider");
        const id = el.getAttribute("data-id");
        return validEmbed(provider, id) ? { provider, id } : false;
      },
    }];
  },
  renderHTML({ node }) {
    const { provider, id } = node.attrs;
    if (!validEmbed(provider, id)) return ["figure", { "data-embed": "", class: "rich-embed" }];
    const url = embedUrl({ provider, id: String(id) });
    return [
      "figure",
      { "data-embed": "", "data-provider": provider, "data-id": id, class: "rich-embed" },
      ["span", { class: "rich-embed-provider" }, `${EMBED_NAMES[provider]} · ${id}`],
      ["span", { class: "rich-embed-url" }, url],
    ];
  },
  addCommands() {
    return {
      setEmbed: (attrs) => ({ commands }) =>
        validEmbed(attrs.provider, attrs.id) ? commands.insertContent({ type: this.name, attrs }) : false,
    };
  },
});

/** Highlighted paragraphs and lists, `info` or `warn` (no table or callout inside). */
export const RichCallout = Node.create({
  name: "callout",
  group: "block",
  content: "(paragraph | bulletList | orderedList)+",
  defining: true,
  addAttributes() {
    return {
      tone: {
        default: "info",
        parseHTML: (el) => (CALLOUT_TONES.includes(el.getAttribute("data-tone") as CalloutTone) ? el.getAttribute("data-tone") : "info"),
        renderHTML: (attrs) => ({ "data-tone": attrs.tone }),
      },
    };
  },
  parseHTML() {
    return [{ tag: "aside[data-callout]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["aside", mergeAttributes(HTMLAttributes, { "data-callout": "", class: "rich-callout" }), 0];
  },
  addCommands() {
    return {
      toggleCallout: (tone) => ({ editor, commands }) => {
        if (!editor.isActive(this.name)) {
          // The selected blocks, a list as a whole (never a part of a list item).
          return commands.command(({ state, tr, dispatch }) => {
            const { $from, $to } = state.selection;
            const range = $from.blockRange($to, (n) => !["bulletList", "orderedList", "listItem"].includes(n.type.name));
            const wrapping = range && findWrapping(range, state.schema.nodes[this.name]!, { tone });
            if (!range || !wrapping) return false;
            // Documents refuse a table or a callout inside a callout: never wrap one.
            let nested = false;
            state.doc.nodesBetween(range.start, range.end, (n) => {
              if (n.type.name === "table" || n.type.name === "callout") nested = true;
              return !nested;
            });
            if (nested) return false;
            if (dispatch) tr.wrap(range, wrapping);
            return true;
          });
        }
        if (editor.isActive(this.name, { tone })) {
          // Unwrapped as a whole: its paragraphs and lists stay as they are.
          return commands.command(({ state, tr, dispatch }) => {
            const callout = findParentNode((n) => n.type.name === this.name)(state.selection);
            if (!callout) return false;
            // All of its content lifted out (a gap-preserving step: the selection and
            // every position inside map as they should).
            const range = state.doc.resolve(callout.pos + 1).blockRange(state.doc.resolve(callout.pos + callout.node.nodeSize - 1));
            const target = range && liftTarget(range);
            if (!range || target === null || target === undefined) return false;
            if (dispatch) tr.lift(range, target);
            return true;
          });
        }
        return commands.updateAttributes(this.name, { tone });
      },
    };
  },
});
