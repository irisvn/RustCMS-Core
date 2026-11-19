// Rich text: edited with TipTap, stored as a document (no HTML), shown with
// React elements (no HTML injection). The editor injects no stylesheet
// (`injectCSS: false`) and its nodes render no `style` attribute: its styles
// are in index.css, as the CSP requires.
import { findParentNode } from "@tiptap/core";
import { type EditorState, NodeSelection, TextSelection, type Transaction } from "@tiptap/pm/state";
import { TableMap } from "@tiptap/pm/tables";
import { EditorContent, useEditor, useEditorState } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import {
  BetweenHorizontalEnd, BetweenVerticalEnd, Bold, Code, Columns3, Heading2, Heading3, Heading4, ImagePlus, Info, Italic, Link, List,
  ListOrdered, Minus, PencilLine, Puzzle, Quote, Rows3, Settings2, Strikethrough, Subscript, Superscript, Table as TableIcon, TableRowsSplit,
  TriangleAlert, Underline, Video, X,
} from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import type * as React from "react";
import type { MediaItem, RichNodeSchema } from "@/api/types";
import { DropdownMenu, DropdownMenuItem } from "@/components/ui/menu";
import { usePanelConfig } from "@/api/context";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input, Label, Textarea } from "@/components/ui/input";
import {
  type DocNode, EMBED_NAMES, type Embed, IMAGE_WIDTHS, type ImageWidth, LIMITS, type RichDocument, asDocument, asShownDocument,
  checkAttrs, embedUrl, isAppNode, isEmptyDocument, mediaUrl, parseEmbedUrl, safeHref, toDocument, toEditor, validEmbed, withoutUnknownElements,
} from "@/lib/rich-text";
import { AppNode, AppNodeFields, RichSubscript, RichSuperscript, appNodeExtensions } from "./rich-text-app";
import { MediaImage } from "./media";
import { useImageViewer } from "@/components/lightbox";
import { MediaPicker, usePickerAccess } from "./media-picker";
import { UploadList, useUploads } from "./uploads";
import { RichCallout, RichEmbed, RichImage, RichTable, RichTableCell, RichTableHeader, RichTableRow } from "./rich-text-nodes";
import { later, useT } from "@/i18n";

/** Most images inserted from the library at once. */
const PICK_MAX = 20;

/** No application node (a field without `richNodes`). */
const NO_NODES: RichNodeSchema[] = [];

/** What the element dialog edits: a node to insert (`position` null) or the one at `position`. */
interface ElementEdit {
  decl: RichNodeSchema;
  attrs: Record<string, unknown>;
  position: number | null;
}

/** What the image dialog edits. */
interface ImageDetails {
  alt: string;
  caption: string;
  size: ImageWidth;
}

export function RichEditorWidget({ id, label, value, onChange, disabled, invalid, describedBy, nullable, onBusy, nodes = NO_NODES, collection, toolbar }: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  invalid: boolean;
  describedBy?: string;
  nullable: boolean;
  onBusy?: (busy: boolean) => void;
  /** The application nodes the field accepts (version 3). */
  nodes?: RichNodeSchema[];
  /** The media collection its images are uploaded into and its picker opens on. */
  collection?: string;
  /**
   * `compact` (repeater rows, ADR 0044): bold, italic, underline, superscript,
   * subscript, images and application elements. The document's rules are the field's either way.
   */
  toolbar?: "compact";
}) {
  const t = useT();
  const full = toolbar !== "compact";
  // The compact toolbar still offers what the selection holds: pasted or stored
  // formatting stays editable and removable.
  const shown = (active: boolean) => full || active;
  const { apiPath } = usePanelConfig();
  const access = usePickerAccess();
  const [picking, setPicking] = useState(false);
  // The link dialog: the address it starts from, and whether a link is being edited.
  const [linking, setLinking] = useState<{ href: string; editing: boolean } | null>(null);
  const [describing, setDescribing] = useState<ImageDetails | null>(null);
  const [embedding, setEmbedding] = useState(false);
  const [element, setElement] = useState<ElementEdit | null>(null);
  // Double-clicking an application node edits it (set once the editor exists).
  const editElement = useRef<(position: number) => void>(() => {});
  // The element being edited, mapped through every change while its dialog is
  // open (uploads may insert before it); null once it is gone.
  const elementAt = useRef<number | null>(null);
  // Images dropped or pasted: inserted in the order they were added, after
  // the selection's block (never replacing it).
  const anchor = useRef<number | null>(null);
  const insertImages = useRef<(items: MediaItem[], chained: boolean) => void>(() => {});
  const uploads = useUploads({
    onUploaded: (item) => insertImages.current([item], true),
    check: (file) => (file.type.startsWith("image/") ? null : later("rich.imagesOnlyFile", { file: file.name })),
    collection,
  });
  // Files still queued or uploading (failures left in the list do not count).
  const sending = uploads.uploads.some((u) => u.status === "queued" || u.status === "uploading");
  const pending = useRef(sending);
  pending.current = sending;
  // Files dropped or pasted: their images go after the block where they were added
  // (that position is kept, mapped through later changes, until they are in).
  const addFiles = useRef((files: File[]) => uploads.add(files));
  // Without the right to upload, files dropped or pasted are ignored (with a message).
  const [refused, setRefused] = useState(false);
  addFiles.current = (files) => {
    if (!access.upload) return setRefused(true);
    setRefused(false);
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
  const stored = value ? asDocument(JSON.parse(value), nodes) : null;
  // The value the editor shows: what it emitted last, or what it opened with.
  // Another value (a form reaction's patch) is drawn without emitting a change.
  const shownValue = useRef(value);
  const editor = useEditor({
    injectCSS: false,
    editable: !disabled,
    extensions: [
      StarterKit.configure({
        heading: { levels: [2, 3, 4] },
        link: { openOnClick: false, autolink: false, linkOnPaste: false, protocols: [], HTMLAttributes: { rel: "noopener noreferrer nofollow", target: null } },
      }),
      RichImage.configure({ inline: false, allowBase64: false, apiPath }),
      RichTable,
      RichTableRow,
      RichTableHeader,
      RichTableCell,
      RichEmbed,
      RichCallout,
      RichSuperscript,
      RichSubscript,
      ...appNodeExtensions(nodes, (position) => editElement.current(position)),
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
      setRefused(false);
      const doc = toDocument(editor.getJSON() as DocNode, apiPath, nodes);
      const next = nullable && isEmptyDocument(doc) ? "" : JSON.stringify(doc);
      shownValue.current = next;
      onChange(next);
    },
  });
  useEffect(() => {
    if (!editor || editor.isDestroyed || value === shownValue.current) return;
    const doc = value ? asDocument(JSON.parse(value), nodes) : { type: "doc" as const, version: 2 as const, content: [] };
    // A value the editor cannot hold is shown read-only instead (below).
    if (!doc) return;
    shownValue.current = value;
    editor.commands.setContent(toEditor(doc, apiPath), { emitUpdate: false });
  }, [editor, value, nodes, apiPath]);
  useEffect(() => {
    // Not an edit: no update (it would mark the form changed).
    editor?.setEditable(!disabled, false);
  }, [editor, disabled]);
  // Where the next uploaded image goes: after the block where the files were
  // added, then after the previous image (mapped through every change).
  useEffect(() => {
    if (!editor) return;
    const follow = ({ transaction }: { transaction: Transaction }) => {
      if (anchor.current !== null) anchor.current = transaction.mapping.map(anchor.current, 1);
      if (elementAt.current !== null) {
        const mapped = transaction.mapping.mapResult(elementAt.current, 1);
        elementAt.current = mapped.deleted ? null : mapped.pos;
      }
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
    // The media's own alternative text, else none to fill in (a file name describes nothing).
    const nodes = items.map((item) => state.schema.nodes.image!.create({ src: mediaUrl(apiPath, item.id), alt: item.alt ?? "" }));
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
        marks: {
          bold: active("bold"),
          italic: active("italic"),
          strike: active("strike"),
          link: active("link"),
          underline: active("underline"),
          superscript: active("superscript"),
          subscript: active("subscript"),
        },
        // The application node selected, if any (its position, for editing).
        element: e.state.selection instanceof NodeSelection && isAppNode(e.state.selection.node.type.name)
          ? e.state.selection.from
          : null,
        code: active("code") || active("codeBlock"),
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

  // A stored value the editor cannot hold: shown, never rewritten. A document
  // (with elements the field does not declare) as a document, anything else as it is.
  if (value && !stored) {
    const shown = asShownDocument(JSON.parse(value));
    if (shown) {
      // Elements the field no longer accepts: removing them makes the rest editable.
      const cleaned = disabled ? null : withoutUnknownElements(shown, nodes);
      return (
        <div id={id} tabIndex={-1} className="rounded-md border border-border p-3" data-fallback="true">
          <p className="mb-2 text-xs text-muted-foreground">{t("rich.readOnlyElements")}</p>
          {cleaned && (
            <Button type="button" variant="outline" size="sm" className="mb-3" onClick={() => onChange(JSON.stringify(cleaned))}>
              {t("rich.removeUnknown")}
            </Button>
          )}
          <RichTextView doc={shown} nodes={nodes} />
        </div>
      );
    }
    return <pre id={id} tabIndex={-1} className="overflow-x-auto rounded-md bg-muted p-3 text-xs" data-fallback="true">{value}</pre>;
  }

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
  // Application nodes: inserted with their defaults, or edited where they are.
  editElement.current = (position) => {
    if (!editor || editor.isDestroyed) return;
    const node = editor.state.doc.nodeAt(position);
    const decl = node && nodes.find((n) => n.name === node.type.name);
    if (!node || !decl) return;
    elementAt.current = position;
    setElement({ decl, attrs: { ...node.attrs }, position });
  };
  const applyElement = (attrs: Record<string, unknown>) => {
    const editing = element;
    setElement(null);
    if (!editor || editor.isDestroyed || !editing) return;
    if (editing.position === null) {
      const content = { type: editing.decl.name, attrs };
      // A selected node (an image, an element) stays: the new element goes after it.
      const { selection } = editor.state;
      return void (selection instanceof NodeSelection ? chain().insertContentAt(selection.to, content) : chain().insertContent(content)).run();
    }
    // Where the element is now (null: removed meanwhile, nothing to change).
    const position = elementAt.current;
    elementAt.current = null;
    const node = position === null ? null : editor.state.doc.nodeAt(position);
    if (position === null || node?.type.name !== editing.decl.name) return;
    editor.view.dispatch(editor.state.tr.setNodeMarkup(position, undefined, attrs));
    editor.commands.focus();
  };
  const s = state;
  const table = s?.table ?? null;
  // Table cells and callouts hold paragraphs (and lists) only: no rule, table or embed there.
  const contained = !!table || !!s?.callout;
  return (
    <div className="min-w-0">
      <div role="toolbar" aria-label={t("rich.toolbar", { field: label })} className="flex flex-wrap gap-1 rounded-t-md border border-border bg-muted/40 p-1">
        {tool(t("rich.bold"), <Bold />, !!s?.marks.bold, () => chain().toggleBold().run())}
        {tool(t("rich.italic"), <Italic />, !!s?.marks.italic, () => chain().toggleItalic().run())}
        {shown(!!s?.marks.strike) && tool(t("rich.strike"), <Strikethrough />, !!s?.marks.strike, () => chain().toggleStrike().run())}
        {tool(t("rich.underline"), <Underline />, !!s?.marks.underline, () => chain().toggleUnderline().run())}
        {tool(t("rich.superscript"), <Superscript />, !!s?.marks.superscript, () => chain().toggleSuperscript().run())}
        {tool(t("rich.subscript"), <Subscript />, !!s?.marks.subscript, () => chain().toggleSubscript().run())}
        {shown(s?.heading === 2) && tool(t("rich.h2"), <Heading2 />, s?.heading === 2, () => chain().toggleHeading({ level: 2 }).run())}
        {shown(s?.heading === 3) && tool(t("rich.h3"), <Heading3 />, s?.heading === 3, () => chain().toggleHeading({ level: 3 }).run())}
        {shown(s?.heading === 4) && tool(t("rich.h4"), <Heading4 />, s?.heading === 4, () => chain().toggleHeading({ level: 4 }).run())}
        {shown(!!s?.bulletList) && tool(t("rich.bullets"), <List />, !!s?.bulletList, () => chain().toggleBulletList().run())}
        {shown(!!s?.orderedList) && tool(t("rich.numbers"), <ListOrdered />, !!s?.orderedList, () => chain().toggleOrderedList().run())}
        {shown(!!s?.blockquote) && tool(t("rich.quote"), <Quote />, !!s?.blockquote, () => chain().toggleBlockquote().run())}
        {shown(!!s?.codeBlock) && tool(t("rich.code"), <Code />, !!s?.codeBlock, () => chain().toggleCodeBlock().run())}
        {full && tool(t("rich.rule"), <Minus />, false, () => chain().setHorizontalRule().run(), contained)}
        {shown(s?.callout === "info") && tool(t("rich.calloutInfo"), <Info />, s?.callout === "info", () => chain().toggleCallout("info").run())}
        {shown(s?.callout === "warn") && tool(t("rich.calloutWarn"), <TriangleAlert />, s?.callout === "warn", () => chain().toggleCallout("warn").run())}
        {shown(!!s?.marks.link) && tool(t("rich.link"), <Link />, !!s?.marks.link, link)}
        {tool(t("rich.image"), <ImagePlus />, false, () => setPicking(true), !access.library && !access.upload)}
        {tool(t("rich.imageDetails"), <Settings2 />, false, describe, !s?.image)}
        {full && tool(t("rich.table"), <TableIcon />, false, () => chain().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(), contained)}
        {full && tool(t("rich.embed"), <Video />, false, () => setEmbedding(true), contained)}
        {nodes.length > 0 && (
          <DropdownMenu
            variant="ghost"
            size="icon"
            aria-label={t("rich.insertElement")}
            trigger={<Puzzle />}
          >
            {nodes.map((decl) => (
              <DropdownMenuItem
                key={decl.name}
                // Blocks never go in table cells or callouts; nothing goes in code.
                disabled={disabled || !editor || !!s?.code || (!decl.inline && contained)}
                onSelect={() => setElement({ decl, attrs: checkAttrs(decl, {}) ?? {}, position: null })}
              >
                {decl.label}
              </DropdownMenuItem>
            ))}
          </DropdownMenu>
        )}
        {nodes.length > 0 && tool(t("rich.editElement"), <PencilLine />, false, () => s?.element != null && editElement.current(s.element), s?.element == null)}
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
        if (!access.upload) return setRefused(true);
        // The images go where the files were dropped.
        const at = editor.view.posAtCoords({ left: e.clientX, top: e.clientY });
        if (at) editor.view.dispatch(editor.state.tr.setSelection(TextSelection.near(editor.state.doc.resolve(at.pos))));
        addFiles.current(files);
      }}>
        <EditorContent editor={editor} />
      </div>
      <p role="status" className="text-xs text-destructive">{refused ? t("rich.uploadRefused") : ""}</p>
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
        rules={{ multiple: true, max: PICK_MAX, accept: ["image/*"], collection }}
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
      <Dialog open={element !== null} onClose={() => {
        setElement(null);
        editor?.commands.focus();
      }} title={element?.decl.label ?? ""}>
        {element && <AppNodeFields decl={element.decl} initial={element.attrs} onClose={() => setElement(null)} onApply={applyElement} />}
      </Dialog>
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
              <input type="radio" className="admin-radio size-4 shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background" name={`${id}-size`} value={w} checked={size === w} onChange={() => setSize(w)} />
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

/** A video address, kept as its provider and id (YouTube, Vimeo); previewed as a text card. */
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

/** An embed as the panel shows it: the provider, the id and its page as text; no iframe, no third-party image. */
function EmbedCard({ embed, link = false }: { embed: Embed; link?: boolean }) {
  const url = embedUrl(embed);
  return (
    <figure className="rich-embed" data-provider={embed.provider}>
      <span className="rich-embed-provider">{EMBED_NAMES[embed.provider]} · {embed.id}</span>
      {link ? <a href={url} rel="noopener noreferrer nofollow" className="rich-embed-url">{url}</a> : <span className="rich-embed-url">{url}</span>}
    </figure>
  );
}

/**
 * A stored document rendered with React elements only. Application nodes are
 * drawn by their extension (`declared`: the field's), else as placeholders;
 * an undeclared one, or one whose attributes do not fit, as "unknown element".
 */
export function RichTextView({ doc, nodes: declared = NO_NODES }: { doc: RichDocument; nodes?: RichNodeSchema[] }) {
  const { apiPath } = usePanelConfig();
  // Its images, in order: each opens the viewer over all of them.
  const images: DocNode[] = [];
  // As drawn: application nodes show no content of theirs.
  const collect = (list: DocNode[] | undefined) => {
    for (const n of list ?? []) {
      if (n.type === "image" && typeof n.attrs?.media === "string") images.push(n);
      if (!isAppNode(n.type)) collect(n.content);
    }
  };
  collect(doc.content);
  const viewer = useImageViewer(images.map((n) => ({ id: String(n.attrs?.media), alt: String(n.attrs?.alt ?? "") || undefined })));
  const view = (node: DocNode) => viewer.open(images.indexOf(node));
  const nodes = (list: DocNode[] | undefined): React.ReactNode =>
    (list ?? []).map((n, i) => <RichNode key={i} node={n} nodes={nodes} apiPath={apiPath} declared={declared} view={view} />);
  return <div className="rich-text">{nodes(doc.content)}{viewer.viewer}</div>;
}

function RichNode({ node, nodes, apiPath, declared, view }: {
  node: DocNode;
  nodes: (list?: DocNode[]) => React.ReactNode;
  apiPath: string;
  declared: RichNodeSchema[];
  view: (node: DocNode) => void;
}) {
  const t = useT();
  if (isAppNode(node.type)) {
    const decl = declared.find((d) => d.name === node.type);
    const attrs = decl ? checkAttrs(decl, node.attrs) : null;
    const known = decl && attrs ? decl : undefined;
    const shown = <AppNode decl={known} name={node.type} attrs={attrs ?? undefined} />;
    return known && !known.inline ? <div className="rich-app-block">{shown}</div> : shown;
  }
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
          <MediaImage src={mediaUrl(apiPath, String(node.attrs?.media))} alt={String(node.attrs?.alt ?? "")}
            onOpen={typeof node.attrs?.media === "string" ? () => view(node) : undefined} buttonClassName="rich-image-open"
            label={t("viewer.open", { name: String(node.attrs?.alt ?? "") || t("viewer.image") })} />
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
    case "tableRow":
    case "tableHeader":
    case "tableCell":
      // Only inside a table (drawn there): out of place, an unknown part.
      return <AppNode name={node.type} attrs={undefined} />;
    case "text": {
      let out: React.ReactNode = node.text ?? "";
      for (const mark of node.marks ?? []) {
        if (mark.type === "bold") out = <strong>{out}</strong>;
        else if (mark.type === "italic") out = <em>{out}</em>;
        else if (mark.type === "strike") out = <s>{out}</s>;
        else if (mark.type === "code") out = <code>{out}</code>;
        else if (mark.type === "underline") out = <u>{out}</u>;
        else if (mark.type === "superscript") out = <sup>{out}</sup>;
        else if (mark.type === "subscript") out = <sub>{out}</sub>;
        else if (mark.type === "link" && safeHref(String(mark.attrs?.href ?? ""))) {
          out = <a href={String(mark.attrs?.href)} rel="noopener noreferrer nofollow" className="underline">{out}</a>;
        }
      }
      return <>{out}</>;
    }
    default:
      // A part this panel does not know: a placeholder, never dropped silently.
      return <AppNode name={node.type} attrs={undefined} />;
  }
}
