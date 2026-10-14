// Rich text: edited with TipTap, stored as a document (no HTML), shown with
// React elements (no HTML injection). The editor injects no stylesheet
// (`injectCSS: false`): its styles are in index.css, as the CSP requires.
import Image from "@tiptap/extension-image";
import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { Bold, Code, Heading2, Heading3, ImagePlus, Italic, Link, List, ListOrdered, Quote, Strikethrough } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type * as React from "react";
import { ApiError } from "@/api/client";
import { usePanelConfig, useSession } from "@/api/context";
import { Button } from "@/components/ui/button";
import { type DocNode, type RichDocument, asDocument, isEmptyDocument, mediaUrl, safeHref, toDocument, toEditor } from "@/lib/rich-text";
import { uploadMedia } from "./media";
import { errorText, later, show, type Text, useT } from "@/i18n";

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
  const session = useSession();
  const picker = useRef<HTMLInputElement>(null);
  const [error, setError] = useState<Text | null>(null);
  const [uploading, setUploading] = useState(false);
  // An image upload with an unknown outcome: retried with the same file and key.
  const [retry, setRetry] = useState<{ file: File; key: string } | null>(null);
  useEffect(() => {
    onBusy?.(uploading || retry !== null);
  }, [uploading, retry, onBusy]);
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
      Image.configure({ inline: false, allowBase64: false }),
    ],
    content: toEditor(stored ?? { type: "doc", version: 1, content: [] }, apiPath),
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
    },
    onUpdate: ({ editor }) => {
      const doc = toDocument(editor.getJSON() as DocNode, apiPath);
      onChange(nullable && isEmptyDocument(doc) ? "" : JSON.stringify(doc));
    },
  });
  useEffect(() => {
    editor?.setEditable(!disabled);
  }, [editor, disabled]);

  // A stored value that is not a document: shown as it is, never rewritten.
  if (value && !stored) return <pre id={id} tabIndex={-1} className="overflow-x-auto rounded-md bg-muted p-3 text-xs" data-fallback="true">{value}</pre>;

  const tool = (name: string, icon: React.ReactNode, active: boolean, run: () => void) => (
    // Keeps the editor's focus and selection while clicking the toolbar.
    <Button key={name} size="icon" variant={active ? "secondary" : "ghost"} aria-label={name} aria-pressed={active} disabled={disabled || !editor} onMouseDown={(e) => e.preventDefault()} onClick={run}>
      {icon}
    </Button>
  );
  const chain = () => editor!.chain().focus();
  const link = () => {
    const href = window.prompt(t("rich.linkPrompt"), editor?.getAttributes("link").href ?? "");
    if (href === null) return;
    if (href === "") return void chain().unsetLink().run();
    if (!safeHref(href)) return setError(() => later("rich.linkInvalid"));
    setError(null);
    chain().setLink({ href }).run();
  };
  const image = async (file: File | undefined, key: string = crypto.randomUUID()) => {
    if (!file || !editor) return;
    if (!file.type.startsWith("image/")) return setError(() => later("rich.imagesOnly"));
    setError(null);
    setUploading(true);
    try {
      const item = await uploadMedia(session.api, file, key);
      setRetry(null);
      if (!editor.isDestroyed) chain().setImage({ src: mediaUrl(apiPath, item.id), alt: item.name }).run();
    } catch (e) {
      if (e instanceof ApiError && e.maybeApplied) setRetry({ file, key });
      setError(() => errorText(e, "media.uploadFailed"));
    } finally {
      setUploading(false);
    }
  };
  const on = (name: string, attrs?: Record<string, unknown>) => editor?.isActive(name, attrs) ?? false;
  return (
    <div className="min-w-0">
      <div role="toolbar" aria-label={t("rich.toolbar", { field: label })} className="flex flex-wrap gap-1 rounded-t-md border border-border bg-muted/40 p-1">
        {tool(t("rich.bold"), <Bold />, on("bold"), () => chain().toggleBold().run())}
        {tool(t("rich.italic"), <Italic />, on("italic"), () => chain().toggleItalic().run())}
        {tool(t("rich.strike"), <Strikethrough />, on("strike"), () => chain().toggleStrike().run())}
        {tool(t("rich.h2"), <Heading2 />, on("heading", { level: 2 }), () => chain().toggleHeading({ level: 2 }).run())}
        {tool(t("rich.h3"), <Heading3 />, on("heading", { level: 3 }), () => chain().toggleHeading({ level: 3 }).run())}
        {tool(t("rich.bullets"), <List />, on("bulletList"), () => chain().toggleBulletList().run())}
        {tool(t("rich.numbers"), <ListOrdered />, on("orderedList"), () => chain().toggleOrderedList().run())}
        {tool(t("rich.quote"), <Quote />, on("blockquote"), () => chain().toggleBlockquote().run())}
        {tool(t("rich.code"), <Code />, on("codeBlock"), () => chain().toggleCodeBlock().run())}
        {tool(t("rich.link"), <Link />, on("link"), link)}
        {tool(t("rich.image"), <ImagePlus />, false, () => picker.current?.click())}
        <input ref={picker} type="file" hidden accept="image/png,image/jpeg,image/gif,image/webp" onChange={(e) => {
          void image(e.target.files?.[0]);
          e.target.value = "";
        }} />
      </div>
      <EditorContent editor={editor} />
      {retry && (
        <Button size="sm" className="mt-2" disabled={disabled || uploading} onClick={() => void image(retry.file, retry.key)}>
          {t("rich.retryImage")}
        </Button>
      )}
      {error && <p role="alert" className="mt-1 text-xs text-destructive">{show(error)}</p>}
    </div>
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
    case "image":
      return <img src={mediaUrl(apiPath, String(node.attrs?.media))} alt={String(node.attrs?.alt ?? "")} className="max-w-full rounded-md" />;
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
