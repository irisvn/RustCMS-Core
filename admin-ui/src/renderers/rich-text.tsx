// Rich text: edited with TipTap, stored as a document (no HTML), shown with
// React elements (no HTML injection). The editor injects no stylesheet
// (`injectCSS: false`) and its nodes render no `style` attribute: its styles
// are in index.css, as the CSP requires.
import { findParentNode } from "@tiptap/core";
import { type EditorState, TextSelection, type Transaction } from "@tiptap/pm/state";
import { TableMap } from "@tiptap/pm/tables";
import { EditorContent, useEditor, useEditorState } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import {
  BetweenHorizontalEnd, BetweenVerticalEnd, Bold, Code, Columns3, Heading2, Heading3, Heading4, ImagePlus, Info, Italic, Link, List,
  ListOrdered, Minus, Quote, Rows3, Settings2, Strikethrough, Table as TableIcon, TableRowsSplit, TriangleAlert, Video, X,
} from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import type * as React from "react";
import type { MediaItem } from "@/api/types";
import { usePanelConfig } from "@/api/context";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input, Label, Textarea } from "@/components/ui/input";
import {
  type DocNode, EMBED_NAMES, type Embed, IMAGE_WIDTHS, type ImageWidth, LIMITS, type RichDocument, asDocument, embedUrl,
  isEmptyDocument, mediaUrl, parseEmbedUrl, safeHref, toDocument, toEditor, validEmbed,
} from "@/lib/rich-text";
import { MediaPicker, usePickerAccess } from "./media-picker";
import { UploadList, useUploads } from "./uploads";
import { RichCallout, RichEmbed, RichImage, RichTable, RichTableCell, RichTableHeader, RichTableRow } from "./rich-text-nodes";
import { later, useT } from "@/i18n";

/** Most images inserted from the library at once. */
const PICK_MAX = 20;

/** What the image dialog edits. */
interface ImageDetails {
  alt: string;
  caption: string;
  size: ImageWidth;
}

export function RichEditorWidget({ id, label, value, onChange, disabled, invalid, describedBy, nullable, onBusy }: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  invalid: boolean;
  describedBy?: string;
  nullable: boolean;
  onBusy?: (busy: boolean) => void;
}) {
  const t = useT();
  const { apiPath } = usePanelConfig();
  const access = usePickerAccess();
  const [picking, setPicking] = useState(false);
  // The link dialog: the address it starts from, and whether a link is being edited.
  const [linking, setLinking] = useState<{ href: string; editing: boolean } | null>(null);
  const [describing, setDescribing] = useState<ImageDetails | null>(null);
  const [embedding, setEmbedding] = useState(false);
  // Images dropped or pasted: inserted in the order they were added, after
  // the selection's block (never replacing it).
  const anchor = useRef<number | null>(null);
  const insertImages = useRef<(items: MediaItem[], chained: boolean) => void>(() => {});
  const uploads = useUploads({
    onUploaded: (item) => insertImages.current([item], true),
    check: (file) => (file.type.startsWith("image/") ? null : later("rich.imagesOnlyFile", { file: file.name })),
  });
  // Files still queued or uploading (failures left in the list do not count).
  const sending = uploads.uploads.some((u) => u.status === "queued" || u.status === "uploading");
  const pending = useRef(sending);
  pending.current = sending;
  // Files dropped or pasted: their images go after the block where they were added
  // (that position is kept, mapped through later changes, until they are in).
  const addFiles = useRef((files: File[]) => uploads.add(files));
  addFiles.current = (files) => {
    // A new batch (nothing else on its way) starts where it is added; one added
    // while another is on its way follows it.
    if ((!pending.current || anchor.current === null) && editor && !editor.isDestroyed) anchor.current = blockEnd(editor.state);
    uploads.add(files);
  };
  // The form may not save while an upload is pending or its outcome unknown.
  useEffect(() => {
    onBusy?.(uploads.busy);
  }, [uploads.busy, onBusy]);
  useEffect(() => () => onBusy?.(false), [onBusy]);
  const stored = value ? asDocument(JSON.parse(value)) : null;
  const editor = useEditor({
    injectCSS: false,
    editable: !disabled,
    extensions: [
      StarterKit.configure({
        heading: { levels: [2, 3, 4] },
        underline: false,
        link: { openOnClick: false, autolink: false, linkOnPaste: false, protocols: [], HTMLAttributes: { rel: "noopener noreferrer nofollow", target: null } },
      }),
      RichImage.configure({ inline: false, allowBase64: false, apiPath }),
      RichTable,
      RichTableRow,
      RichTableHeader,
      RichTableCell,
      RichEmbed,
      RichCallout,
    ],
    content: toEditor(stored ?? { type: "doc", version: 2, content: [] }, apiPath),
    editorProps: {
      attributes: {
        id,
        role: "textbox",
        "aria-multiline": "true",
        "aria-label": label,
        ...(describedBy ? { "aria-describedby": describedBy } : {}),
        ...(invalid ? { "aria-invalid": "true" } : {}),
        class: "rich-text min-h-40 rounded-b-md border border-t-0 border-border bg-background px-3 py-2 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
      },
      // Files pasted are uploaded (the HTML that comes with them, external images included, is not used).
      handlePaste: (_view, event) => {
        const files = [...(event.clipboardData?.files ?? [])];
        if (files.length === 0) return false;
        event.preventDefault();
        addFiles.current(files);
        return true;
      },
    },
    onUpdate: ({ editor }) => {
      const doc = toDocument(editor.getJSON() as DocNode, apiPath);
      onChange(nullable && isEmptyDocument(doc) ? "" : JSON.stringify(doc));
    },
  });
  useEffect(() => {
    editor?.setEditable(!disabled);
  }, [editor, disabled]);
  // Where the next uploaded image goes: after the block where the files were
  // added, then after the previous image (mapped through every change).
  useEffect(() => {
    if (!editor) return;
    const follow = ({ transaction }: { transaction: Transaction }) => {
      if (anchor.current !== null) anchor.current = transaction.mapping.map(anchor.current, 1);
    };
    editor.on("transaction", follow);
    return () => {
      editor.off("transaction", follow);
    };
  }, [editor]);
  useEffect(() => {
    if (!sending) anchor.current = null;
  }, [sending]);
  // `chained`: after the previous upload of the same batch.
  insertImages.current = (items, chained) => {
    if (!editor || editor.isDestroyed || items.length === 0) return;
    const { state, view } = editor;
    const nodes = items.map((item) => state.schema.nodes.image!.create({ src: mediaUrl(apiPath, item.id), alt: item.name }));
    const fresh = !chained || anchor.current === null || anchor.current > state.doc.content.size;
    const at = fresh ? blockEnd(state) : anchor.current!;
    // The selection stays the user's: typing on never replaces an image that arrived meanwhile.
    view.dispatch(state.tr.insert(at, nodes));
    if (chained) anchor.current = at + nodes.reduce((size, n) => size + n.nodeSize, 0);
  };
  // What the toolbar shows: follows the selection (TipTap does not re-render on transactions).
  const state = useEditorState({
    editor,
    selector: ({ editor: e }) => {
      if (!e) return null;
      const table = findParentNode((n) => n.type.name === "table")(e.state.selection);
      const map = table ? TableMap.get(table.node) : null;
      const active = (name: string, attrs?: Record<string, unknown>) => e.isActive(name, attrs);
      return {
        marks: { bold: active("bold"), italic: active("italic"), strike: active("strike"), link: active("link") },
        heading: [2, 3, 4].find((level) => active("heading", { level })) ?? null,
        bulletList: active("bulletList"),
        orderedList: active("orderedList"),
        blockquote: active("blockquote"),
        codeBlock: active("codeBlock"),
        image: active("image"),
        callout: active("callout") ? String(e.getAttributes("callout").tone) : null,
        table: map ? { rows: map.height, columns: map.width, header: table!.node.firstChild?.firstChild?.type.name === "tableHeader" } : null,
      };
    },
  });

  // A stored value that is not a document: shown as it is, never rewritten.
  if (value && !stored) return <pre id={id} tabIndex={-1} className="overflow-x-auto rounded-md bg-muted p-3 text-xs" data-fallback="true">{value}</pre>;

  const tool = (name: string, icon: React.ReactNode, active: boolean, run: () => void, off = false) => (
    // Keeps the editor's focus and selection while clicking the toolbar.
    <Button key={name} size="icon" variant={active ? "secondary" : "ghost"} aria-label={name} aria-pressed={active} disabled={disabled || !editor || off} onMouseDown={(e) => e.preventDefault()} onClick={run}>
      {icon}
    </Button>
  );
  const chain = () => editor!.chain().focus();
  // The link dialog edits the link at the selection (or the one the cursor is in).
  const link = () => {
    if (!editor) return;
    setLinking({ href: String(editor.getAttributes("link").href ?? ""), editing: editor.isActive("link") });
  };
  const applyLink = (href: string) => {
    setLinking(null);
    if (!editor || editor.isDestroyed) return;
    if (href === "") return void chain().extendMarkRange("link").unsetLink().run();
    if (editor.state.selection.empty && !editor.isActive("link")) {
      // Nothing selected: the address becomes the linked text.
      return void chain().insertContent({ type: "text", text: href, marks: [{ type: "link", attrs: { href } }] }).run();
    }
    chain().extendMarkRange("link").setLink({ href }).run();
  };
  const describe = () => {
    if (!editor?.isActive("image")) return;
    const a = editor.getAttributes("image");
    setDescribing({ alt: String(a.alt ?? ""), caption: String(a.caption ?? ""), size: IMAGE_WIDTHS.includes(a.size) ? a.size : "normal" });
  };
  const applyDetails = (details: ImageDetails) => {
    setDescribing(null);
    if (!editor || editor.isDestroyed || !editor.isActive("image")) return;
    chain().updateAttributes("image", { ...details }).run();
  };
  const applyEmbed = (embed: Embed) => {
    setEmbedding(false);
    if (!editor || editor.isDestroyed) return;
    chain().setEmbed(embed).run();
  };
  const s = state;
  const table = s?.table ?? null;
  return (
    <div className="min-w-0">
      <div role="toolbar" aria-label={t("rich.toolbar", { field: label })} className="flex flex-wrap gap-1 rounded-t-md border border-border bg-muted/40 p-1">
        {tool(t("rich.bold"), <Bold />, !!s?.marks.bold, () => chain().toggleBold().run())}
        {tool(t("rich.italic"), <Italic />, !!s?.marks.italic, () => chain().toggleItalic().run())}
        {tool(t("rich.strike"), <Strikethrough />, !!s?.marks.strike, () => chain().toggleStrike().run())}
        {tool(t("rich.h2"), <Heading2 />, s?.heading === 2, () => chain().toggleHeading({ level: 2 }).run())}
        {tool(t("rich.h3"), <Heading3 />, s?.heading === 3, () => chain().toggleHeading({ level: 3 }).run())}
        {tool(t("rich.h4"), <Heading4 />, s?.heading === 4, () => chain().toggleHeading({ level: 4 }).run())}
        {tool(t("rich.bullets"), <List />, !!s?.bulletList, () => chain().toggleBulletList().run())}
        {tool(t("rich.numbers"), <ListOrdered />, !!s?.orderedList, () => chain().toggleOrderedList().run())}
        {tool(t("rich.quote"), <Quote />, !!s?.blockquote, () => chain().toggleBlockquote().run())}
        {tool(t("rich.code"), <Code />, !!s?.codeBlock, () => chain().toggleCodeBlock().run())}
        {tool(t("rich.rule"), <Minus />, false, () => chain().setHorizontalRule().run())}
        {tool(t("rich.calloutInfo"), <Info />, s?.callout === "info", () => chain().toggleCallout("info").run())}
        {tool(t("rich.calloutWarn"), <TriangleAlert />, s?.callout === "warn", () => chain().toggleCallout("warn").run())}
        {tool(t("rich.link"), <Link />, !!s?.marks.link, link)}
        {tool(t("rich.image"), <ImagePlus />, false, () => setPicking(true), !access.library && !access.upload)}
        {tool(t("rich.imageDetails"), <Settings2 />, false, describe, !s?.image)}
        {tool(t("rich.table"), <TableIcon />, false, () => chain().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(), !!table)}
        {tool(t("rich.embed"), <Video />, false, () => setEmbedding(true))}
      </div>
      {table && (
        <div role="toolbar" aria-label={t("rich.tableTools")} className="flex flex-wrap items-center gap-1 border border-t-0 border-border bg-muted/20 p-1">
          {tool(t("rich.rowAdd"), <BetweenHorizontalEnd />, false, () => chain().addRowAfter().run(), table.rows >= LIMITS.tableRows)}
          {tool(t("rich.columnAdd"), <BetweenVerticalEnd />, false, () => chain().addColumnAfter().run(), table.columns >= LIMITS.tableColumns)}
          {tool(t("rich.rowRemove"), <TableRowsSplit />, false, () => chain().deleteRow().run(), table.rows <= 1)}
          {tool(t("rich.columnRemove"), <Columns3 />, false, () => chain().deleteColumn().run(), table.columns <= 1)}
          {tool(t("rich.headerRow"), <Rows3 />, table.header, () => chain().toggleHeaderRow().run())}
          {tool(t("rich.tableRemove"), <X />, false, () => chain().deleteTable().run())}
          <span className="px-2 text-xs text-muted-foreground">
            {t("rich.tableSize", { rows: table.rows, columns: table.columns, maxRows: LIMITS.tableRows, maxColumns: LIMITS.tableColumns })}
          </span>
        </div>
      )}
      {/* Files dropped on the editor are uploaded, before ProseMirror sees the drop. */}
      <div onDropCapture={(e) => {
        const files = [...(e.dataTransfer?.files ?? [])];
        if (files.length === 0 || disabled || !editor) return;
        e.preventDefault();
        e.stopPropagation();
        // The images go where the files were dropped.
        const at = editor.view.posAtCoords({ left: e.clientX, top: e.clientY });
        if (at) editor.view.dispatch(editor.state.tr.setSelection(TextSelection.near(editor.state.doc.resolve(at.pos))));
        addFiles.current(files);
      }}>
        <EditorContent editor={editor} />
      </div>
      <UploadList uploads={uploads.uploads} onRetry={uploads.retry} onDismiss={uploads.dismiss} disabled={disabled} />
      <LinkDialog state={linking} onClose={() => {
        setLinking(null);
        editor?.commands.focus();
      }} onApply={applyLink} />
      <Dialog open={describing !== null} onClose={() => {
        setDescribing(null);
        editor?.commands.focus();
      }} title={t("rich.imageDetails")}>
        {describing && <ImageFields initial={describing} onClose={() => setDescribing(null)} onApply={applyDetails} />}
      </Dialog>
      <MediaPicker
        open={picking}
        title={t("rich.imageTitle")}
        action={t("picker.insert")}
        rules={{ multiple: true, max: PICK_MAX, accept: ["image/*"] }}
        onClose={() => {
          setPicking(false);
          editor?.commands.focus();
        }}
        onPick={(items) => {
          setPicking(false);
          insertImages.current(items, false);
          editor?.commands.focus();
        }}
      />
      <Dialog open={embedding} onClose={() => {
        setEmbedding(false);
        editor?.commands.focus();
      }} title={t("rich.embed")}>
        {embedding && <EmbedFields onClose={() => setEmbedding(false)} onApply={applyEmbed} />}
      </Dialog>
    </div>
  );
}

/** The position after the selection's block, in the nearest parent that may hold an image (not a table cell or a callout). */
function blockEnd(state: EditorState): number {
  const $to = state.selection.$to;
  const image = state.schema.nodes.image!;
  for (let d = $to.depth; d > 0; d--) {
    const index = $to.index(d - 1) + 1;
    if ($to.node(d - 1).canReplaceWith(index, index, image)) return $to.after(d);
  }
  return $to.depth === 0 ? $to.pos : state.doc.content.size;
}

/** Enter in a text field applies the dialog (never submits the record's form), except while composing. */
const onEnter = (apply: () => void) => (e: React.KeyboardEvent) => {
  if (e.nativeEvent.isComposing || e.nativeEvent.keyCode === 229) return;
  if (e.key === "Enter") {
    e.preventDefault();
    apply();
  }
};

/**
 * The address of a link: checked as the server checks it (`http`, `https`,
 * `mailto`). Not a <form>: the editor sits inside the record's form, so Enter
 * is handled here and never submits it.
 */
function LinkDialog({ state, onClose, onApply }: {
  state: { href: string; editing: boolean } | null;
  onClose: () => void;
  onApply: (href: string) => void;
}) {
  const t = useT();
  return (
    <Dialog open={state !== null} onClose={onClose} title={state?.editing ? t("rich.linkEdit") : t("rich.linkAdd")}>
      {state && <LinkFields key={state.href} initial={state.href} editing={state.editing} onClose={onClose} onApply={onApply} />}
    </Dialog>
  );
}

function LinkFields({ initial, editing, onClose, onApply }: {
  initial: string;
  editing: boolean;
  onClose: () => void;
  onApply: (href: string) => void;
}) {
  const t = useT();
  const id = useId();
  const [href, setHref] = useState(initial);
  const [invalid, setInvalid] = useState(false);
  const apply = () => {
    const value = href.trim();
    if (value !== "" && !safeHref(value)) return setInvalid(true);
    onApply(value);
  };
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <Label htmlFor={id}>{t("rich.linkAddress")}</Label>
        <Input
          id={id}
          type="url"
          inputMode="url"
          autoFocus
          placeholder={t("rich.linkPlaceholder")}
          value={href}
          aria-invalid={invalid || undefined}
          aria-describedby={invalid ? `${id}-error` : undefined}
          onChange={(e) => {
            setHref(e.target.value);
            setInvalid(false);
          }}
          onKeyDown={onEnter(apply)}
        />
        {invalid && <p id={`${id}-error`} role="alert" className="text-xs leading-5 text-destructive">{t("rich.linkInvalid")}</p>}
      </div>
      <div className="flex flex-wrap justify-end gap-2">
        {editing && (
          <Button type="button" variant="ghost" className="mr-auto" onClick={() => onApply("")}>
            {t("rich.linkRemove")}
          </Button>
        )}
        <Button type="button" variant="outline" onClick={onClose}>{t("action.cancel")}</Button>
        <Button type="button" onClick={apply}>{t("rich.linkApply")}</Button>
      </div>
    </div>
  );
}

/** An image's alternative text, caption and width (the server's limits, in code points). */
function ImageFields({ initial, onClose, onApply }: { initial: ImageDetails; onClose: () => void; onApply: (d: ImageDetails) => void }) {
  const t = useT();
  const id = useId();
  const [alt, setAlt] = useState(initial.alt);
  const [caption, setCaption] = useState(initial.caption);
  const [size, setSize] = useState<ImageWidth>(initial.size);
  const apply = () => onApply({ alt: [...alt].slice(0, LIMITS.alt).join(""), caption: [...caption.trim()].slice(0, LIMITS.caption).join(""), size });
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <Label htmlFor={`${id}-alt`}>{t("rich.imageAlt")}</Label>
        <Textarea id={`${id}-alt`} autoFocus rows={2} value={alt} aria-describedby={`${id}-alt-help`} onChange={(e) => setAlt(e.target.value)} />
        <p id={`${id}-alt-help`} className="text-xs text-muted-foreground">{t("rich.imageAltHelp", { max: LIMITS.alt })}</p>
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor={`${id}-caption`}>{t("rich.imageCaption")}</Label>
        <Input id={`${id}-caption`} value={caption} onChange={(e) => setCaption(e.target.value)} onKeyDown={onEnter(apply)} />
      </div>
      <fieldset className="flex flex-col gap-2">
        <legend className="mb-2 text-sm font-medium">{t("rich.imageWidth")}</legend>
        <div className="flex flex-wrap gap-4">
          {IMAGE_WIDTHS.map((w) => (
            <label key={w} className="inline-flex items-center gap-2 text-sm">
              <input type="radio" name={`${id}-size`} value={w} checked={size === w} onChange={() => setSize(w)} />
              {t(`rich.width.${w}`)}
            </label>
          ))}
        </div>
      </fieldset>
      <div className="flex flex-wrap justify-end gap-2">
        <Button type="button" variant="outline" onClick={onClose}>{t("action.cancel")}</Button>
        <Button type="button" onClick={apply}>{t("rich.linkApply")}</Button>
      </div>
    </div>
  );
}

/** A video or post address, kept as its provider and id (YouTube, Vimeo, X); previewed as a card. */
function EmbedFields({ onClose, onApply }: { onClose: () => void; onApply: (embed: Embed) => void }) {
  const t = useT();
  const id = useId();
  const [address, setAddress] = useState("");
  const [invalid, setInvalid] = useState(false);
  const embed = parseEmbedUrl(address);
  const apply = () => (embed ? onApply(embed) : setInvalid(true));
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <Label htmlFor={id}>{t("rich.embedAddress")}</Label>
        <Input
          id={id}
          type="url"
          inputMode="url"
          autoFocus
          placeholder={t("rich.embedPlaceholder")}
          value={address}
          aria-invalid={invalid || undefined}
          aria-describedby={`${id}-help`}
          onChange={(e) => {
            setAddress(e.target.value);
            setInvalid(false);
          }}
          onKeyDown={onEnter(apply)}
        />
        <p id={`${id}-help`} className={invalid ? "text-xs text-destructive" : "text-xs text-muted-foreground"} role={invalid ? "alert" : undefined}>
          {invalid ? t("rich.embedInvalid") : t("rich.embedHelp")}
        </p>
      </div>
      {embed && <EmbedCard embed={embed} />}
      <div className="flex flex-wrap justify-end gap-2">
        <Button type="button" variant="outline" onClick={onClose}>{t("action.cancel")}</Button>
        <Button type="button" onClick={apply}>{t("rich.embedInsert")}</Button>
      </div>
    </div>
  );
}

/** An embed as the panel shows it: the provider and its page, no iframe and no third-party image. */
function EmbedCard({ embed, link = false }: { embed: Embed; link?: boolean }) {
  const url = embedUrl(embed);
  return (
    <figure className="rich-embed" data-provider={embed.provider}>
      <span className="rich-embed-provider">{EMBED_NAMES[embed.provider]}</span>
      {link ? <a href={url} rel="noopener noreferrer nofollow" className="rich-embed-url">{url}</a> : <span className="rich-embed-url">{url}</span>}
    </figure>
  );
}

/** A stored document rendered with React elements only. */
export function RichTextView({ doc }: { doc: RichDocument }) {
  const { apiPath } = usePanelConfig();
  const nodes = (list: DocNode[] | undefined): React.ReactNode =>
    (list ?? []).map((n, i) => <RichNode key={i} node={n} nodes={nodes} apiPath={apiPath} />);
  return <div className="rich-text">{nodes(doc.content)}</div>;
}

function RichNode({ node, nodes, apiPath }: { node: DocNode; nodes: (list?: DocNode[]) => React.ReactNode; apiPath: string }) {
  switch (node.type) {
    case "paragraph":
      return <p>{nodes(node.content)}</p>;
    case "heading": {
      const level = Number(node.attrs?.level);
      return level === 3 ? <h3>{nodes(node.content)}</h3> : level === 4 ? <h4>{nodes(node.content)}</h4> : <h2>{nodes(node.content)}</h2>;
    }
    case "bulletList":
      return <ul>{nodes(node.content)}</ul>;
    case "orderedList":
      return <ol>{nodes(node.content)}</ol>;
    case "listItem":
      return <li>{nodes(node.content)}</li>;
    case "blockquote":
      return <blockquote>{nodes(node.content)}</blockquote>;
    case "codeBlock":
      return <pre><code>{(node.content ?? []).map((c) => c.text ?? "").join("")}</code></pre>;
    case "horizontalRule":
      return <hr />;
    case "hardBreak":
      return <br />;
    case "image": {
      const caption = String(node.attrs?.caption ?? "");
      const width = IMAGE_WIDTHS.includes(node.attrs?.width as ImageWidth) ? String(node.attrs?.width) : "normal";
      return (
        <figure className="rich-figure" data-width={width}>
          <img src={mediaUrl(apiPath, String(node.attrs?.media))} alt={String(node.attrs?.alt ?? "")} />
          {caption && <figcaption>{caption}</figcaption>}
        </figure>
      );
    }
    case "table": {
      const rows = node.content ?? [];
      const header = rows[0]?.content?.[0]?.type === "tableHeader";
      const row = (r: DocNode, i: number, Cell: "th" | "td") => <tr key={i}>{(r.content ?? []).map((c, j) => <Cell key={j}>{nodes(c.content)}</Cell>)}</tr>;
      return (
        <div className="rich-table-scroll">
          <table className="rich-table">
            {header && <thead>{row(rows[0]!, 0, "th")}</thead>}
            <tbody>{rows.slice(header ? 1 : 0).map((r, i) => row(r, i, "td"))}</tbody>
          </table>
        </div>
      );
    }
    case "embed":
      return validEmbed(node.attrs?.provider, node.attrs?.id) ? <EmbedCard embed={{ provider: node.attrs.provider, id: String(node.attrs.id) }} link /> : null;
    case "callout":
      return <aside className="rich-callout" data-tone={node.attrs?.tone === "warn" ? "warn" : "info"}>{nodes(node.content)}</aside>;
    case "text": {
      let out: React.ReactNode = node.text ?? "";
      for (const mark of node.marks ?? []) {
        if (mark.type === "bold") out = <strong>{out}</strong>;
        else if (mark.type === "italic") out = <em>{out}</em>;
        else if (mark.type === "strike") out = <s>{out}</s>;
        else if (mark.type === "code") out = <code>{out}</code>;
        else if (mark.type === "link" && safeHref(String(mark.attrs?.href ?? ""))) {
          out = <a href={String(mark.attrs?.href)} rel="noopener noreferrer nofollow" className="underline">{out}</a>;
        }
      }
      return <>{out}</>;
    }
    default:
      return null;
  }
}
