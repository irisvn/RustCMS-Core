// Renders an infolist (ADR 0039): a record's view page, or a panel page's
// data, drawn by entries in the form's layouts. Unknown (newer) entry types
// fall back to text instead of breaking the page.
import { Check, Copy } from "lucide-react";
import { useMemo, useState } from "react";
import type * as React from "react";
import { Link } from "@tanstack/react-router";
import type { ComponentSchema, Condition, Data, EntrySchema, InfolistSchema, JsonValue } from "@/api/types";
import { Badge, badgeColor } from "@/components/ui/badge";
import { ScriptView } from "@/components/custom";
import { cn } from "@/lib/utils";
import { asShownDocument } from "@/lib/rich-text";
import type { DocNode } from "@/lib/rich-text";
import { formatWireDate, useLocale, useT } from "@/i18n";
import { colorOf, isDateKind, optionLabel } from "@/lib/wire";
import { formConditions, hasConditions, hiddenFields } from "@/schema/conditions";
import { defaultValues, inputsOf, isEntry, isLayout, isTextObject, wireValues } from "@/schema/form";
import { SchemaLayout, SplitLayout, TabsLayout, always, gridCols, readOnlyText, shownWith, span, withShownChildren } from "./form";
import { MediaThumb } from "./media";
import { RichTextView } from "./rich-text";

export const KNOWN_ENTRIES = new Set([
  "text_entry",
  "relation_entry",
  "boolean_entry",
  "key_value_entry",
  "media_entry",
  "rich_text_entry",
  "computed_entry",
  "custom_entry",
  "repeatable_entry",
]);

/** Computed values of a view page: loading, or the server's answer. */
export type ComputedState = { status: "loading" } | { status: "error" } | { status: "ready"; data: Data; failed: string[] };

interface Context {
  data: Data;
  /**
   * The record's form (view pages): the kinds and conditions of fields the
   * infolist does not show, which its conditions may name (a hidden input).
   */
  form?: ComponentSchema[];
  /** Titles of related records (`Envelope.labels`). */
  labels?: Record<string, string>;
  computed?: ComputedState;
  /** The resource and record (custom entries); absent on panel pages. */
  slug?: string;
  recordKey?: string;
}

/** The SDK's `Infolist` (`ui` 3): entries over `data`, in their grid. */
export function Infolist({ schema, data, className }: { schema: InfolistSchema; data: Data; className?: string }) {
  return (
    <div className={cn("grid grid-cols-1 gap-x-6 gap-y-6", gridCols(schema.columns), className)}>
      <InfolistRenderer infolist={schema} data={data} />
    </div>
  );
}

/** Whether `components` show a computed value (the view page then loads them). */
export function hasComputed(components: ComponentSchema[]): boolean {
  return components.some((c) => (isLayout(c) ? hasComputed(c.components) : isEntry(c) && c.computed === true));
}

/**
 * An infolist's components, its conditions evaluated on `data`: the stored values for a
 * declared infolist (ADR 0039), as the edit form reads them for one made from the form.
 */
export function InfolistRenderer({ infolist, ...ctx }: { infolist: InfolistSchema } & Context) {
  const { components, fromForm } = infolist;
  const { form, data } = ctx;
  const shown = useMemo(() => {
    if (!hasConditions(components)) return always;
    const own = formConditions(components);
    if (!form) return shownWith(own, data as Record<string, unknown>);
    // Each field with its kind (the form's for fields the infolist does not show).
    const edit = formConditions(form);
    const conditions = { fields: own.fields, kinds: new Map([...edit.kinds, ...own.kinds]) };
    if (!fromForm) return shownWith(conditions, data as Record<string, unknown>);
    // Made from the form: the values the edit form would hold (empty for fields not
    // sent, such as write-only ones), null where the form hides them.
    const inputs = inputsOf(form);
    const values: Record<string, unknown> = wireValues(inputs, defaultValues(inputs, data as Record<string, unknown>));
    for (const name of hiddenFields(edit, values)) values[name] = null;
    return shownWith(conditions, values);
  }, [components, fromForm, form, data]);
  return <Body components={components} ctx={ctx} shown={shown} />;
}

function Body({ components, ctx, shown }: {
  components: ComponentSchema[];
  ctx: Context;
  shown: (condition: Condition | undefined) => boolean;
}) {
  return <>{components.map((component, index) => {
    if (!shown(component.shownWhen)) return null;
    if (isLayout(component)) {
      const body = (layout: { components: ComponentSchema[] }) => <Body components={layout.components} ctx={ctx} shown={shown} />;
      if (component.type === "tabs") {
        return <TabsLayout key={`layout-${index}`} layout={component} shown={shown} render={body} />;
      }
      if (component.type === "split") {
        return <SplitLayout key={`layout-${index}`} layout={withShownChildren(component, shown)} render={body} />;
      }
      return <SchemaLayout key={`layout-${index}`} layout={component}>{body(component)}</SchemaLayout>;
    }
    if (!isEntry(component)) return null;
    return (
      <dl key={`${component.name}-${index}`} className={cn("min-w-0 space-y-2", span(component.columnSpan))}>
        <dt className="text-sm font-medium text-muted-foreground">{component.label}</dt>
        <dd className="select-text break-words text-sm leading-6" data-fallback={!KNOWN_ENTRIES.has(component.type) || undefined}>
          <EntryValue entry={component} ctx={ctx} />
        </dd>
        {component.help && <p className="text-xs text-muted-foreground">{component.help}</p>}
      </dl>
    );
  })}</>;
}

/** A value an entry cannot draw its way, as text (JSON indented, line breaks kept). */
function Raw({ value }: { value: JsonValue }) {
  return <span className="whitespace-pre-wrap">{readOnlyText(value)}</span>;
}

function Muted({ children }: { children: React.ReactNode }) {
  return <span className="text-muted-foreground">{children}</span>;
}

function EntryValue({ entry, ctx }: { entry: EntrySchema; ctx: Context }) {
  const t = useT();
  const supplied = Object.prototype.hasOwnProperty.call(ctx.data, entry.name);
  // A computed value (a custom entry may draw one too).
  const state = ctx.computed;
  if (state && entry.computed) {
    if (state.status === "loading") {
      return <span role="status" className="inline-block h-5 w-24 animate-pulse rounded bg-muted" aria-label={t("entry.loading")} />;
    }
    if (state.status === "error" || state.failed.includes(entry.name) || !(entry.name in state.data)) {
      return <Muted>{t("entry.notAvailable")}</Muted>;
    }
    return <Shown entry={entry} value={state.data[entry.name] as JsonValue} ctx={ctx} />;
  }
  if (!supplied || ctx.data[entry.name] === undefined) return <Muted>{t("value.notProvided")}</Muted>;
  return <Shown entry={entry} value={ctx.data[entry.name] as JsonValue} ctx={ctx} />;
}

function Shown({ entry, value, ctx }: { entry: EntrySchema; value: JsonValue; ctx: Context }) {
  const t = useT();
  if (entry.type === "custom_entry" && entry.widget) {
    return (
      <ScriptView kind="entry" name={entry.widget} data={value} record={ctx.data} props={entry.props}
        place={{ kind: "entry", resource: ctx.slug ?? "", field: entry.name }}
        fallback={<TextValue entry={entry} value={value} />} />
    );
  }
  if (value === null) return <Muted>{t("value.null")}</Muted>;
  switch (entry.type) {
    case "relation_entry":
      return <RelationValue entry={entry} value={value} ctx={ctx} />;
    case "boolean_entry":
      return <BooleanValue entry={entry} value={value} />;
    case "key_value_entry":
      return isTextObject(value) ? <KeyValues value={value} /> : <Raw value={value} />;
    case "media_entry": {
      const ids = (Array.isArray(value) ? value : [value]).filter((v): v is string => typeof v === "string");
      return <div className="flex flex-wrap gap-3">{ids.map((id) => <MediaThumb key={id} id={id} size={entry.size} link={entry.gallery} />)}</div>;
    }
    case "repeatable_entry": {
      // Each row shown by the entry's own entries, over the row's values.
      if (!Array.isArray(value)) return <Raw value={value} />;
      if (value.length === 0) return <Muted>{t("repeater.empty")}</Muted>;
      const rows = { columns: 2, components: entry.components ?? [] };
      return (
        <ol className="flex flex-col gap-3">
          {value.map((row, i) => (
            <li key={i} className="rounded-lg border border-border p-3">
              <div className="grid grid-cols-1 gap-x-6 gap-y-4 md:grid-cols-2">
                <InfolistRenderer infolist={rows} slug={ctx.slug} recordKey={ctx.recordKey}
                  data={(row && typeof row === "object" && !Array.isArray(row) ? row : {}) as Data} />
              </div>
            </li>
          ))}
        </ol>
      );
    }
    case "rich_text_entry": {
      // Never raw JSON: a value that is not a document says so.
      const doc = asShownDocument(value);
      // A legacy value that is text (stored before the field held documents): shown as it is.
      if (!doc && typeof value === "string") return <Raw value={value} />;
      if (!doc) return <p className="text-sm text-muted-foreground">{t("rich.cannotShow")}</p>;
      return <>{entry.outline && <Outline nodes={doc.content} />}<RichTextView doc={doc} nodes={entry.richNodes} /></>;
    }
    default:
      // Text, computed and unknown (newer) entries.
      return <TextValue entry={entry} value={value} />;
  }
}

function RelationValue({ entry, value, ctx }: { entry: EntrySchema; value: JsonValue; ctx: Context }) {
  const key = typeof value === "string" || typeof value === "number" ? String(value) : null;
  if (key === null) return <Raw value={value} />;
  const label = ctx.labels?.[entry.name] ?? `#${key}`;
  // `#key`: the user may not view the related record.
  if (!entry.link || !entry.relationship || label === `#${key}`) return <span>{label}</span>;
  return (
    <Link to="/r/$slug/records/$key" params={{ slug: entry.relationship.resource, key }} className="font-medium text-primary underline-offset-4 hover:underline">
      {label}
    </Link>
  );
}

function BooleanValue({ entry, value }: { entry: EntrySchema; value: JsonValue }) {
  const t = useT();
  if (typeof value !== "boolean") return <Raw value={value} />;
  const label = entry.labels ? entry.labels[value ? 0 : 1] : value ? t("value.yes") : t("value.no");
  const color = colorOf(entry.colors, value);
  if (!color) return <>{label}</>;
  return <Badge color={badgeColor(color)}>{label}</Badge>;
}

function KeyValues({ value }: { value: Record<string, string> }) {
  const t = useT();
  const entries = Object.entries(value);
  if (entries.length === 0) return <Muted>{t("value.noEntries")}</Muted>;
  return (
    <dl className="grid grid-cols-[minmax(0,auto)_minmax(0,1fr)] gap-x-4 gap-y-1">
      {entries.map(([k, v]) => [
        <dt key={`k-${k}`} className="font-medium">{k}</dt>,
        <dd key={`v-${k}`} className="break-words">{v}</dd>,
      ])}
    </dl>
  );
}

/** The headings of a document, as a list (`RichTextEntry::outline`). */
function Outline({ nodes }: { nodes?: DocNode[] }) {
  const t = useT();
  const text = (node: DocNode): string => node.text ?? (node.content ?? []).map(text).join("");
  const headings = (nodes ?? []).filter((n) => n.type === "heading").map((n) => ({ level: Number(n.attrs?.level ?? 1), text: text(n) }));
  if (headings.length === 0) return null;
  return (
    <nav aria-label={t("entry.outline")} className="mb-3 rounded-md border border-border p-3">
      <ul className="space-y-1">
        {headings.map((h, i) => <li key={i} style={{ paddingInlineStart: `${(Math.min(h.level, 4) - 1) * 0.75}rem` }}>{h.text}</li>)}
      </ul>
    </nav>
  );
}

/** Text, numbers, dates, option labels and lists, with the entry's options. */
function TextValue({ entry, value }: { entry: EntrySchema; value: JsonValue }) {
  const t = useT();
  const locale = useLocale();
  if (value === null) return <Muted>{t("value.null")}</Muted>;
  if (value === "") return <Muted>{t("value.emptyString")}</Muted>;
  if (entry.password) return <span aria-label={t("value.hidden")}>••••••••</span>;
  const label = (item: JsonValue): string => {
    const option = optionLabel(entry.options, item);
    if (option !== undefined) return option;
    if (typeof item === "boolean") return item ? t("value.yes") : t("value.no");
    if (typeof item === "string" && isDateKind(entry.valueType)) {
      return formatWireDate(entry.valueType, item) ?? item;
    }
    if (entry.decimals !== undefined && (typeof item === "number" || typeof item === "string")) {
      const n = Number(item);
      // Integers beyond what a double holds exactly are shown as written.
      if (Number.isFinite(n) && (typeof item === "number" || Math.abs(n) <= Number.MAX_SAFE_INTEGER)) {
        // The server allows 20 decimals at most; a schema asking more shows the value as written.
        const decimals = Math.min(Math.max(entry.decimals, 0), 20);
        return new Intl.NumberFormat(locale, { minimumFractionDigits: decimals, maximumFractionDigits: decimals }).format(n);
      }
    }
    return readOnlyText(item);
  };
  const items = Array.isArray(value) ? value : [value];
  if (Array.isArray(value) && value.length === 0) return <Muted>{t("value.emptyList")}</Muted>;
  const one = (item: JsonValue, key?: number) => <Item key={key} entry={entry} item={item} text={label(item)} />;
  if (Array.isArray(value) && (entry.list || entry.badge)) {
    return entry.badge
      ? <div className="flex flex-wrap gap-1.5">{items.map((item, i) => one(item, i))}</div>
      : <ul className="space-y-1">{items.map((item, i) => <li key={i}>{one(item)}</li>)}</ul>;
  }
  // Records in a list: one JSON value, indented (joined, they would read as one line).
  if (Array.isArray(value) && value.some((item) => item !== null && typeof item === "object")) return one(value);
  if (Array.isArray(value)) return <>{items.map(label).join(", ")}</>;
  return one(value);
}

function Item({ entry, item, text }: { entry: EntrySchema; item: JsonValue; text: string }) {
  const limited = entry.limit !== undefined && [...text].length > entry.limit ? `${[...text].slice(0, entry.limit).join("")}…` : text;
  // Dates and times as the user reads them, the wire value for machines.
  const shown = typeof item === "string" && isDateKind(entry.valueType) ? <time dateTime={item}>{limited}</time> : limited;
  let content: React.ReactNode = shown;
  if (entry.badge) {
    const color = colorOf(entry.colors, item);
    content = <Badge color={badgeColor(color)}>{shown}</Badge>;
  } else if (entry.url && typeof item === "string" && /^https?:\/\//i.test(item)) {
    content = <a href={item} target="_blank" rel="noopener noreferrer" className="text-primary underline-offset-4 hover:underline">{limited}</a>;
  }
  return (
    <span className={cn("inline-flex max-w-full items-center gap-1.5", entry.multiline && "whitespace-pre-wrap")} title={limited !== text ? text : undefined}>
      {entry.prefix && <Muted>{entry.prefix}</Muted>}
      <span className="min-w-0 break-words">{content}</span>
      {entry.suffix && <Muted>{entry.suffix}</Muted>}
      {entry.copyable && <CopyButton text={text} />}
    </span>
  );
}

function CopyButton({ text }: { text: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      if (!navigator.clipboard) throw new Error("no clipboard");
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  };
  return (
    <button type="button" onClick={() => void copy()} aria-label={copied ? t("entry.copied") : t("entry.copy")}
      className="inline-flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring">
      {copied ? <Check className="size-3.5" aria-hidden="true" /> : <Copy className="size-3.5" aria-hidden="true" />}
    </button>
  );
}
