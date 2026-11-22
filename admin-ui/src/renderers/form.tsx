// Renders a form schema: layouts and inputs from the vocabulary. Unknown types
// fall back to a read-only rendering instead of breaking the page.
import { Controller, useWatch, type Control, type FieldErrors } from "react-hook-form";
import { ChevronDown, Plus, X } from "lucide-react";
import { createContext, useContext, useEffect, useId, useMemo, useRef, useState } from "react";
import type * as React from "react";
import type { ComponentSchema, Condition, InputSchema, LayoutSchema, SecretStatus, SelectOption } from "@/api/types";
import {
  type FormConditions,
  type WireValues,
  conditionNames,
  formConditions,
  hasConditions,
  hiddenFields,
  holds,
} from "@/schema/conditions";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox, Switch } from "@/components/ui/checkbox";
import { Input, Label, NativeSelect, Textarea } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { UNREADABLE_NUMBER, type WireKind } from "@/lib/wire";
import {
  encodePair,
  hasUnsafeInteger,
  inputsOf,
  isInput,
  isLayout,
  isTriState,
  pairsOf,
  type FormValue,
  type FormValues,
  wireValues,
} from "@/schema/form";
import { RelationSelect } from "./relation-select";
import { LinksWidget } from "./links";
import { RepeaterWidget } from "./repeater";
import { RelationRepeaterWidget } from "./relation-repeater";
import { FileUploadWidget } from "./media";
import { RichEditorWidget } from "./rich-text";
import { CustomField } from "./custom";
import { type SecretControl, SecretWidget } from "./secret";
import { isResourceSecret, parseSecret, parseSecretMap, type SecretRow } from "@/schema/secrets";
import { useT } from "@/i18n";

export interface FormContext {
  slug: string;
  /** Key of the edited or viewed record (options are authorized on it). */
  record?: string;
  control: Control<FormValues>;
  errors: FieldErrors<FormValues>;
  /** View mode: everything read-only. */
  readOnly?: boolean;
  /** Called when the user changes an input (blind edits submit exactly these). */
  onUserChange?: (name: string) => void;
  /** Submit attempts so far: each failed one opens the tab of its first error. */
  submitCount?: number;
  /** An input starts or ends background work (an upload): no saving meanwhile. */
  onBusy?: (name: string, busy: boolean) => void;
  /** Secret settings: their stored state and what the user chose to do with them. */
  secrets?: SecretControl;
  /**
   * Where a relationship select reads its options (default: the resource
   * field's options). Action forms and relation dialogs have their own.
   */
  optionsPath?: (field: string) => string;
  /** Warnings under JSON repeater rows, from the form's reactions (ADR 0043). */
  rowFlags?: import("@/schema/reactions").RowFlags;
}

export const KNOWN_LAYOUTS = new Set(["section", "grid", "tabs", "tab", "split", "pane"]);

const SPAN: Record<number, string> = {
  1: "md:col-span-1",
  2: "md:col-span-2",
  3: "md:col-span-3",
  4: "md:col-span-4",
};
const COLS: Record<number, string> = {
  1: "md:grid-cols-1",
  2: "md:grid-cols-2",
  3: "md:grid-cols-3",
  4: "md:grid-cols-4",
};
export const gridCols = (n: number) => COLS[Math.min(Math.max(n, 1), 4)] ?? COLS[1];

/** The page width of a record's form (`Form::width`): unknown (newer) widths are the default. */
export const formWidthClass = (width?: string) => (width === "full" ? "max-w-none" : width === "wide" ? "max-w-6xl" : "max-w-4xl");
export const span = (n: number) => SPAN[Math.min(Math.max(n, 1), 4)] ?? SPAN[1];

/**
 * Whether a component's condition holds (ADR 0037), set by the outermost
 * `Components` of a form for the nested ones.
 */
const ShownContext = createContext<((condition: Condition | undefined) => boolean) | null>(null);
export const always = () => true;

/** A predicate on conditions for wire `values` (hidden fields read as null). */
export function shownWith(form: FormConditions, values: WireValues) {
  const hidden = hiddenFields(form, values);
  return (condition: Condition | undefined) => !condition || holds(form, condition, values, hidden);
}

/** A layout with only the children (tabs, panes) its conditions show. */
export const withShownChildren = (layout: LayoutSchema, shown: (c: Condition | undefined) => boolean): LayoutSchema =>
  layout.components.every((c) => shown(c.shownWhen))
    ? layout
    : { ...layout, components: layout.components.filter((c) => shown(c.shownWhen)) };

/** A form's components: its conditions are evaluated here, on every change. */
export function Components({ components, ctx }: { components: ComponentSchema[]; ctx: FormContext }) {
  return hasConditions(components) ? <LiveConditions components={components} ctx={ctx} /> : (
    <ShownContext.Provider value={always}><ComponentList components={components} ctx={ctx} /></ShownContext.Provider>
  );
}

/** Components inside a form (layouts' children), shown by the form's conditions. */
function ComponentList({ components, ctx }: { components: ComponentSchema[]; ctx: FormContext }) {
  const shown = useContext(ShownContext) ?? always;
  return (
    <>
      {components.map((c, i) =>
        // Hidden: not rendered, its draft kept in the form state (never sent).
        !shown(c.shownWhen) ? null
          : isInput(c) ? <Field key={c.name} input={c} ctx={ctx} />
          // Forms hold no entries (refused by the server).
          : isLayout(c) ? <Layout key={`layout-${i}`} layout={c} ctx={ctx} /> : null,
      )}
    </>
  );
}

/**
 * Re-evaluates the form's conditions as the user types, without a request.
 * Only the inputs the conditions read are watched: typing elsewhere does not
 * re-render the form.
 */
function LiveConditions({ components, ctx }: { components: ComponentSchema[]; ctx: FormContext }) {
  const form = useMemo(() => formConditions(components), [components]);
  const names = useMemo(() => conditionNames(components), [components]);
  const read = useMemo(() => inputsOf(components).filter((i) => names.includes(i.name)), [components, names]);
  const watched = useWatch({ control: ctx.control, name: names }) as FormValue[];
  const shown = useMemo(() => {
    const values: FormValues = Object.fromEntries(names.map((n, i) => [n, watched[i]!]).filter(([, v]) => v !== undefined));
    return shownWith(form, wireValues(read, values));
  }, [form, names, read, watched]);
  return <ShownContext.Provider value={shown}><ComponentList components={components} ctx={ctx} /></ShownContext.Provider>;
}

export function SchemaLayout({ layout, errors, attempt, children }: {
  layout: LayoutSchema;
  errors?: FieldErrors<FormValues>;
  attempt?: number;
  children: React.ReactNode;
}) {
  const grid = <div className={cn("grid grid-cols-1", layout.compact ? "gap-x-4 gap-y-4" : "gap-x-6 gap-y-6", gridCols(layout.columns))}>{children}</div>;
  if (layout.type === "section" && layout.collapsed !== undefined) {
    return <CollapsibleSection layout={layout} errors={errors} attempt={attempt}>{grid}</CollapsibleSection>;
  }
  if (layout.type === "section" || !KNOWN_LAYOUTS.has(layout.type)) {
    return (
      <fieldset className={cn("col-span-full min-w-0 rounded-lg border border-border", layout.compact ? "p-4" : "p-5 sm:p-6")}>
        {layout.label && <legend className="px-2 text-sm font-semibold tracking-tight">{layout.label}</legend>}
        {grid}
      </fieldset>
    );
  }
  return <div className="col-span-full min-w-0">{grid}</div>;
}

/**
 * A section the user folds and unfolds (a button with `aria-expanded`). Its
 * inputs stay mounted while folded (drafts are kept); it opens by itself when
 * one of them has an error (after each failed save). Nothing is remembered.
 */
function CollapsibleSection({ layout, errors, attempt, children }: {
  layout: LayoutSchema;
  errors?: FieldErrors<FormValues>;
  attempt?: number;
  children: React.ReactNode;
}) {
  const base = useId();
  const [open, setOpen] = useState(!layout.collapsed);
  const failing = inputsOf(layout.components).some((i) => errors?.[i.name]);
  useEffect(() => {
    if (failing) setOpen(true);
  }, [failing, attempt]);
  return (
    <section aria-labelledby={`${base}-title`} className="col-span-full min-w-0 rounded-lg border border-border">
      <h3 id={`${base}-title`} className="m-0 text-sm font-semibold tracking-tight">
        <button type="button" aria-expanded={open} aria-controls={`${base}-body`} onClick={() => setOpen((o) => !o)}
          className={cn("flex w-full items-center justify-between gap-2 rounded-lg text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", layout.compact ? "p-4" : "p-5 sm:px-6")}>
          {layout.label}
          <ChevronDown aria-hidden="true" className={cn("size-4 shrink-0 text-muted-foreground transition-transform", open && "rotate-180")} />
        </button>
      </h3>
      <div id={`${base}-body`} hidden={!open} className={layout.compact ? "px-4 pb-4" : "px-5 pb-5 sm:px-6 sm:pb-6"}>{children}</div>
    </section>
  );
}

/**
 * A main column and an aside (from `lg`: the aside 20rem wide and sticky
 * under the page's top; narrower: one column, the aside after the main one or
 * before it with `asideFirst`). Either pane may be missing (nothing in it the
 * user may see).
 */
export function SplitLayout({ layout, render }: { layout: LayoutSchema; render: (pane: LayoutSchema) => React.ReactNode }) {
  const pane = (id: string) => layout.components.find((c): c is LayoutSchema => isLayout(c) && c.type === "pane" && c.id === id);
  const main = pane("main");
  const aside = pane("aside");
  const column = (p: LayoutSchema) => <div className={cn("grid min-w-0 grid-cols-1 content-start gap-x-6 gap-y-6", gridCols(p.columns))}>{render(p)}</div>;
  return (
    <div className={cn("col-span-full grid min-w-0 grid-cols-1 gap-6", main && aside && "lg:grid-cols-[minmax(0,1fr)_20rem]")}>
      {main && <div className="min-w-0">{column(main)}</div>}
      {aside && (
        // Sticky below the panel's header (4rem tall), with a gap.
        <aside className={cn("min-w-0 lg:sticky lg:top-20 lg:self-start", layout.asideFirst && "order-first lg:order-none")}>
          {column(aside)}
        </aside>
      )}
    </div>
  );
}

function Layout({ layout, ctx }: { layout: LayoutSchema; ctx: FormContext }) {
  const shown = useContext(ShownContext) ?? always;
  // Tabs and panes are children of their layout: hidden ones are left out.
  if (layout.type === "tabs") {
    return <TabsLayout layout={layout} shown={shown} errors={ctx.errors} attempt={ctx.submitCount} render={(tab) => <ComponentList components={tab.components} ctx={ctx} />} />;
  }
  if (layout.type === "split") {
    return <SplitLayout layout={withShownChildren(layout, shown)} render={(pane) => <ComponentList components={pane.components} ctx={ctx} />} />;
  }
  return (
    <SchemaLayout layout={layout} errors={ctx.errors} attempt={ctx.submitCount}>
      <ComponentList components={layout.components} ctx={ctx} />
    </SchemaLayout>
  );
}

/**
 * WAI-ARIA tabs. Every panel stays mounted (drafts survive switching). After
 * each failed save (and when server errors arrive), the tab holding the first
 * error is shown; nested tabs do the same, so every enclosing tab opens. The
 * form then focuses that input.
 */
export function TabsLayout({ layout, shown = always, errors, attempt, render }: {
  layout: LayoutSchema;
  /** Which tabs the form's conditions show (ADR 0037); the others are left out. */
  shown?: (condition: Condition | undefined) => boolean;
  errors?: FieldErrors<FormValues>;
  attempt?: number;
  render: (tab: LayoutSchema) => React.ReactNode;
}) {
  const t = useT();
  const base = useId();
  // The open tab is remembered by its place among the declared tabs: a tab
  // hidden or shown before it does not move the user to another one, and it
  // is open again when it is shown again.
  const shownTabs = layout.components
    .map((c, declared) => ({ c, declared }))
    .filter((e): e is { c: LayoutSchema; declared: number } => isLayout(e.c) && shown(e.c.shownWhen));
  const tabs = shownTabs.map((e) => e.c);
  const [active, setActive] = useState(() => shownTabs[0]?.declared ?? 0);
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const current = Math.max(shownTabs.findIndex((e) => e.declared === active), 0);
  const failing = tabs.map((tab) => inputsOf(tab.components).filter((i) => errors?.[i.name]).map((i) => i.name));
  const signature = failing.map((names) => names.join(",")).join("|");
  useEffect(() => {
    const index = failing.findIndex((names) => names.length > 0);
    if (index >= 0) setActive(shownTabs[index]!.declared);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature, attempt]);
  const open = (i: number) => setActive(shownTabs[i]!.declared);
  const select = (index: number) => {
    const next = (index + tabs.length) % tabs.length;
    open(next);
    refs.current[next]?.focus();
  };
  const onKey = (e: React.KeyboardEvent) => {
    const keys: Record<string, number> = { ArrowRight: current + 1, ArrowLeft: current - 1, Home: 0, End: tabs.length - 1 };
    if (e.key in keys) {
      e.preventDefault();
      select(keys[e.key]!);
    }
  };
  if (tabs.length === 0) return null;
  return (
    <div className="col-span-full min-w-0">
      <div role="tablist" aria-label={layout.label ?? t("form.sections")} className="flex flex-wrap gap-1 border-b border-border" onKeyDown={onKey}>
        {tabs.map((tab, i) => {
          const id = tab.id ?? String(shownTabs[i]!.declared);
          const count = failing[i]?.length ?? 0;
          return (
            <button
              key={id}
              ref={(el) => {
                refs.current[i] = el;
              }}
              type="button"
              role="tab"
              id={`${base}-tab-${id}`}
              aria-selected={i === current}
              aria-controls={`${base}-panel-${id}`}
              tabIndex={i === current ? 0 : -1}
              onClick={() => open(i)}
              className={cn(
                "-mb-px inline-flex min-h-9 items-center gap-2 border-b-2 px-3 text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                i === current ? "border-foreground text-foreground" : "border-transparent text-muted-foreground hover:text-foreground",
              )}
            >
              {tab.label ?? id}
              {count > 0 && (
                <span className="rounded-full bg-destructive px-1.5 text-xs text-destructive-foreground">
                  {count}
                  <span className="sr-only"> {t("form.errors", { count })}</span>
                </span>
              )}
            </button>
          );
        })}
      </div>
      {tabs.map((tab, i) => {
        const id = tab.id ?? String(shownTabs[i]!.declared);
        return (
          <div key={id} role="tabpanel" id={`${base}-panel-${id}`} aria-labelledby={`${base}-tab-${id}`} hidden={i !== current} className="pt-5">
            <div className={cn("grid grid-cols-1 gap-x-6 gap-y-6", gridCols(tab.columns))}>{render(tab)}</div>
          </div>
        );
      })}
    </div>
  );
}

/** A value as text: JSON indented, the rest as written. */
export function readOnlyText(value: unknown): string {
  return typeof value === "object" ? JSON.stringify(value, null, 2) : String(value);
}

function Field({ input, ctx }: { input: InputSchema; ctx: FormContext }) {
  if (input.type === "hidden") return null;
  const id = `field-${input.name}`;
  const error = ctx.errors[input.name]?.message;
  const describedBy = [input.help && `${id}-help`, error && `${id}-error`].filter(Boolean).join(" ") || undefined;
  const disabled = input.disabled || ctx.readOnly;
  const required = input.rules.some((r) => r.rule === "required");
  return (
    <div className={cn("flex min-w-0 flex-col gap-2", span(input.columnSpan))}>
      {(input.type !== "checkbox" || isTriState(input)) && (
        <Label htmlFor={id}>
          {input.label}
          {required && !disabled && <span className="text-destructive"> *</span>}
        </Label>
      )}
      <Controller
        name={input.name}
        control={ctx.control}
        render={({ field }) => (
          <Widget
            id={id}
            input={input}
            ctx={ctx}
            value={field.value}
            onChange={(value: FormValue) => {
              ctx.onUserChange?.(input.name);
              field.onChange(value);
            }}
            onBlur={field.onBlur}
            disabled={disabled}
            invalid={!!error}
            describedBy={describedBy}
          />
        )}
      />
      {input.help && <p id={`${id}-help`} className="text-xs leading-5 text-muted-foreground">{input.help}</p>}
      {error && (
        <p id={`${id}-error`} role="alert" className="text-xs leading-5 text-destructive">
          {String(error)}
        </p>
      )}
    </div>
  );
}

export interface WidgetProps {
  id: string;
  input: InputSchema;
  ctx: FormContext;
  value: FormValue;
  onChange: (value: FormValue) => void;
  onBlur: () => void;
  disabled?: boolean;
  invalid: boolean;
  describedBy?: string;
}

const encodeOption = (o: SelectOption) => JSON.stringify(o.value);

/** An encoded option value as text (a string without its quotes). */
function optionText(encoded: string): string {
  try {
    const value: unknown = JSON.parse(encoded);
    return typeof value === "string" ? value : encoded;
  } catch {
    return encoded;
  }
}

export function Widget({ id, input, ctx, value, onChange, onBlur, disabled, invalid, describedBy }: WidgetProps) {
  const t = useT();
  if (isTriState(input)) {
    // Nullable boolean: "not set" is a value of its own, and can be restored.
    return (
      <NativeSelect id={id} disabled={disabled} onBlur={onBlur} aria-invalid={invalid || undefined} aria-describedby={describedBy} value={typeof value === "string" ? value : ""} onChange={(e) => onChange(e.target.value)}>
        <option value="">{t("value.notSet")}</option>
        <option value="true">{t("value.yes")}</option>
        <option value="false">{t("value.no")}</option>
      </NativeSelect>
    );
  }
  const common = { id, disabled, onBlur, "aria-invalid": invalid || undefined, "aria-describedby": describedBy, placeholder: input.placeholder };
  const text = typeof value === "string" ? value : "";
  const onText = (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>) =>
    onChange(e.target.value);
  switch (input.type) {
    case "text_input": {
      const type = input.mode === "password" ? "password" : input.mode === "email" ? "email" : input.mode === "url" ? "url" : "text";
      const field = (className?: string, described?: string) => (
        <Input
          {...common}
          aria-describedby={described}
          className={className}
          type={type}
          value={text}
          onChange={onText}
          autoComplete={input.mode === "password" ? "new-password" : undefined}
          placeholder={input.writeOnly ? t("form.keepValue") : input.placeholder}
        />
      );
      if (!input.prefix && !input.suffix) return field(undefined, describedBy);
      // Fixed text inside the input's border: not part of the value, read
      // with the input (aria-describedby).
      const affix = (text: string, which: "prefix" | "suffix") => (
        <span id={`${id}-${which}`} className="flex shrink-0 items-center whitespace-nowrap px-3 text-sm text-muted-foreground">{text}</span>
      );
      const described = [input.prefix && `${id}-prefix`, input.suffix && `${id}-suffix`, describedBy].filter(Boolean).join(" ") || undefined;
      return (
        <div className={cn(
          "flex h-10 min-w-0 w-full items-stretch overflow-hidden rounded-md border border-input bg-card shadow-xs focus-within:ring-2 focus-within:ring-ring",
          invalid && "border-destructive", disabled && "bg-muted text-subtle-foreground",
        )}>
          {input.prefix && affix(input.prefix, "prefix")}
          {field(cn("h-auto rounded-none border-0 shadow-none focus-visible:ring-0", input.prefix && "pl-0", input.suffix && "pr-0"), described)}
          {input.suffix && affix(input.suffix, "suffix")}
        </div>
      );
    }
    case "textarea":
      return <Textarea {...common} value={text} onChange={onText} rows={4} />;
    case "number_input": {
      // i32 and f64 fit a JavaScript number: a native number input with the
      // declared bounds. i64 and decimals stay text, so no digit is lost.
      const kind = input.valueType as WireKind;
      if (kind === "i32" || kind === "f64") {
        const bounds = input.number;
        // Unreadable text: the input keeps what was typed (its value stays
        // ""), the form holds a value that fails its check. Read on every
        // `input` event: React's onChange skips those where the value stays
        // "" (typing `-` into an empty field, clearing unreadable text).
        const read = (e: React.FormEvent<HTMLInputElement>) =>
          onChange(e.currentTarget.validity.badInput ? UNREADABLE_NUMBER : e.currentTarget.value);
        // Phone keypads for digits have no minus: only when the minimum rules negatives out.
        const unsigned = bounds?.min !== undefined && bounds.min >= 0;
        return (
          <Input
            {...common}
            type="number"
            inputMode={unsigned ? (kind === "i32" ? "numeric" : "decimal") : undefined}
            min={bounds?.min}
            max={bounds?.max}
            step={bounds?.step ?? (kind === "i32" ? 1 : "any")}
            value={text === UNREADABLE_NUMBER ? "" : text}
            onInput={read}
            onChange={read}
          />
        );
      }
      return <Input {...common} inputMode="decimal" value={text} onChange={onText} />;
    }
    case "date_picker":
      return <Input {...common} type="date" value={text} onChange={onText} />;
    case "date_time_picker":
      if (input.valueType === "instant") {
        return (
          <div className="flex items-center gap-2">
            <Input {...common} type="datetime-local" step={1} value={text} onChange={onText} />
            <span className="text-xs text-muted-foreground">{t("form.utc")}</span>
          </div>
        );
      }
      return <Input {...common} type="datetime-local" step={1} value={text} onChange={onText} />;
    case "toggle":
      return <Switch {...common} checked={value === true} onCheckedChange={onChange} aria-label={input.label} />;
    case "checkbox":
      return (
        <label className="flex items-center gap-2 text-sm font-medium">
          <Checkbox {...common} checked={value === true} onChange={(e) => onChange(e.target.checked)} />
          {input.label}
        </label>
      );
    case "checkbox_list": {
      const selected = Array.isArray(value) ? value : [];
      return (
        <div id={id} tabIndex={-1} role="group" aria-label={input.label} aria-invalid={invalid || undefined} aria-describedby={describedBy} className="flex flex-col gap-2.5 focus-visible:outline-none">
          {(input.options ?? []).map((o) => {
            const encoded = encodeOption(o);
            return (
              <label key={encoded} className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={selected.includes(encoded)}
                  onBlur={onBlur}
                  aria-invalid={invalid || undefined}
                  aria-describedby={describedBy}
                  disabled={disabled || hasUnsafeInteger(o.value)}
                  onChange={(e) =>
                    onChange(e.target.checked ? [...selected, encoded] : selected.filter((s) => s !== encoded))
                  }
                />
                {o.label}
              </label>
            );
          })}
        </div>
      );
    }
    case "select":
      if (input.relationship) {
        return (
          <RelationSelect
            id={id}
            slug={ctx.slug}
            field={input.name}
            path={ctx.optionsPath?.(input.name)}
            record={ctx.record}
            value={text}
            onChange={onChange}
            disabled={disabled}
            invalid={invalid}
            describedBy={describedBy}
            onBlur={onBlur}
            placeholder={input.placeholder}
          />
        );
      }
      return (
        <NativeSelect {...common} value={text} onChange={onText}>
          <option value="">{input.placeholder ?? "—"}</option>
          {/* A stored value no longer among the options (a collection no longer
              declared, say) shows as it is rather than as a blank choice. */}
          {text !== "" && !(input.options ?? []).some((o) => encodeOption(o) === text) && (
            <option value={text} disabled>
              {optionText(text)}
            </option>
          )}
          {(input.options ?? []).map((o) => (
            <option key={encodeOption(o)} value={encodeOption(o)} disabled={hasUnsafeInteger(o.value)}>
              {o.label}
            </option>
          ))}
        </NativeSelect>
      );
    case "repeater":
      if (input.repeater?.relationship) {
        return <RelationRepeaterWidget id={id} input={input} ctx={ctx} value={value} onChange={onChange} onBlur={onBlur} disabled={disabled} invalid={invalid} describedBy={describedBy} />;
      }
      return <RepeaterWidget id={id} input={input} ctx={ctx} value={value} onChange={onChange} onBlur={onBlur} disabled={disabled} invalid={invalid} describedBy={describedBy} />;
    case "belongs_to_many":
      return <LinksWidget id={id} input={input} slug={ctx.slug} record={ctx.record} value={text} onChange={onChange} disabled={disabled} invalid={invalid} describedBy={describedBy} onBlur={onBlur} />;
    case "file_upload":
      return <FileUploadWidget id={id} input={input} value={value} onChange={onChange} disabled={disabled} invalid={invalid} describedBy={describedBy} onBusy={(busy) => ctx.onBusy?.(input.name, busy)} />;
    case "rich_editor":
      return <RichEditorWidget id={id} label={input.label} value={text} onChange={onChange} disabled={disabled} invalid={invalid} describedBy={describedBy} nullable={input.nullable} onBusy={(busy) => ctx.onBusy?.(input.name, busy)} nodes={input.richNodes} collection={input.collection} />;
    case "custom":
      return <CustomField id={id} input={input} value={text} onChange={onChange} disabled={disabled} invalid={invalid} describedBy={describedBy} />;
    case "secret":
      if (isResourceSecret(input)) {
        // An encrypted field: its state and intent live in the form value.
        const current = parseSecret(value);
        const control: SecretControl = {
          status: { [input.name]: current.state as SecretStatus },
          intents: { [input.name]: current.intent },
          setIntent: (_, intent) => onChange(JSON.stringify({ state: current.state, intent })),
        };
        return <SecretWidget id={id} name={input.name} control={control} disabled={disabled} invalid={invalid} describedBy={describedBy} placeholder={input.placeholder} />;
      }
      return <SecretWidget id={id} name={input.name} control={ctx.secrets} disabled={disabled} invalid={invalid} describedBy={describedBy} placeholder={input.placeholder} />;
    case "secret_map":
      return <SecretMapEditor id={id} label={input.label} nullable={input.nullable} value={value} onChange={onChange} onBlur={onBlur} disabled={disabled} invalid={invalid} describedBy={describedBy} />;
    case "key_value":
      return <KeyValueEditor id={id} label={input.label} nullable={input.nullable} value={value} onChange={onChange} onBlur={onBlur} disabled={disabled} invalid={invalid} describedBy={describedBy} />;
    default:
      // Unknown component (newer vocabulary): show the value, never edit it.
      return <Input {...common} value={Array.isArray(value) ? value.join(", ") : String(value)} disabled readOnly data-fallback="true" />;
  }
}

/**
 * Named secrets: stored keys are kept, replaced or removed (their values are
 * never shown nor prefilled); new rows take a key and a value.
 */
function SecretMapEditor({ id, label, nullable, value, onChange, onBlur, disabled, invalid, describedBy }: {
  id: string;
  label: string;
  nullable: boolean;
  value: FormValue;
  onChange: (value: FormValue) => void;
  onBlur: () => void;
  disabled?: boolean;
  invalid: boolean;
  describedBy?: string;
}) {
  const t = useT();
  const current = parseSecretMap(value);
  const set = (rows: SecretRow[]) => onChange(JSON.stringify({ ...current, rows }));
  const clearing = (clear: boolean) => onChange(JSON.stringify({ ...current, clear }));
  if (current.clear) {
    return (
      <div id={id} tabIndex={-1} role="group" aria-label={label} aria-describedby={describedBy} className="flex flex-wrap items-center gap-2">
        <span className="text-xs text-muted-foreground">{t("secret.willClear")}</span>
        {!disabled && <Button type="button" variant="ghost" size="sm" onClick={() => clearing(false)}>{t("secret.undo")}</Button>}
      </div>
    );
  }
  const edit = (index: number, change: Partial<SecretRow>) =>
    set(current.rows.map((row, i) => (i === index ? { ...row, ...change } : row)));
  return (
    <div id={id} tabIndex={-1} role="group" aria-label={label} aria-invalid={invalid || undefined} aria-describedby={describedBy} className="flex flex-col gap-2 focus-visible:outline-none">
      {current.state === "unreadable" && (
        <Badge className="w-fit border border-destructive bg-background text-destructive">{t("secret.unreadable")}</Badge>
      )}
      {current.rows.length === 0 && <p className="text-sm text-muted-foreground">{t("form.noEntries")}</p>}
      {current.rows.map((row, i) => (
        <div key={i} className="flex min-w-0 flex-wrap items-center gap-2">
          {row.stored ? (
            <span className="min-w-0 flex-1 truncate font-mono text-sm">{row.key}</span>
          ) : (
            <Input aria-label={t("form.key", { n: i + 1 })} value={row.key} disabled={disabled} onBlur={onBlur} onChange={(e) => edit(i, { key: e.target.value })} className="min-w-0 flex-1" />
          )}
          {row.stored && row.action === "keep" && <Badge>{t("secretMap.kept")}</Badge>}
          {row.stored && row.action === "remove" && <span className="text-xs text-muted-foreground">{t("secretMap.removed")}</span>}
          {(!row.stored || row.action === "replace") && (
            <Input
              type="password"
              autoComplete="new-password"
              aria-label={t("form.value", { n: i + 1 })}
              placeholder={t("secret.placeholder")}
              value={row.value}
              disabled={disabled}
              onBlur={onBlur}
              onChange={(e) => edit(i, { value: e.target.value })}
              className="min-w-0 flex-[2]"
            />
          )}
          {!disabled && row.stored && row.action === "keep" && (
            <Button type="button" variant="outline" size="sm" onClick={() => edit(i, { action: "replace" })}>{t("secret.replace")}</Button>
          )}
          {!disabled && row.stored && row.action !== "keep" && (
            <Button type="button" variant="ghost" size="sm" onClick={() => edit(i, { action: "keep", value: "" })}>{t("secret.undo")}</Button>
          )}
          {!disabled && (row.stored ? row.action === "keep" : true) && (
            <Button size="icon" variant="ghost" aria-label={t("form.removeEntry", { n: i + 1 })}
              onClick={() => (row.stored ? edit(i, { action: "remove" }) : set(current.rows.filter((_, j) => j !== i)))}>
              <X />
            </Button>
          )}
        </div>
      ))}
      <div>
        <Button size="sm" variant="outline" disabled={disabled}
          onClick={() => set([...current.rows, { key: "", stored: false, action: "replace", value: "" }])}>
          <Plus /> {t("form.addEntry")}
        </Button>
        {/* Stored (even unreadable) and nullable: it can go back to "not set". */}
        {nullable && current.state !== "unset" && current.revision !== undefined && (
          <Button type="button" size="sm" variant="ghost" className="ml-2" disabled={disabled} onClick={() => clearing(true)}>
            {t("secret.clear")}
          </Button>
        )}
      </div>
    </div>
  );
}

/** Editable key/value rows; duplicates and empty keys are reported by validation. */
function KeyValueEditor({ id, label, nullable, value, onChange, onBlur, disabled, invalid, describedBy }: {
  id: string;
  label: string;
  nullable: boolean;
  value: FormValue;
  onChange: (value: FormValue) => void;
  onBlur: () => void;
  disabled?: boolean;
  invalid: boolean;
  describedBy?: string;
}) {
  const t = useT();
  // A stored value that is not text pairs: shown as is (the input is disabled).
  if (typeof value === "string" && value !== "") {
    return <pre id={id} tabIndex={-1} className="overflow-x-auto rounded-md bg-muted p-3 text-xs" data-fallback="true">{value}</pre>;
  }
  const pairs = pairsOf(value);
  const set = (next: [string, string][]) => onChange(next.map(([k, v]) => encodePair(k, v)));
  const edit = (index: number, part: 0 | 1, text: string) =>
    set(pairs.map((pair, i) => (i === index ? (part === 0 ? [text, pair[1]] : [pair[0], text]) : pair)));
  return (
    <div id={id} tabIndex={-1} role="group" aria-label={label} aria-invalid={invalid || undefined} aria-describedby={describedBy} className="flex flex-col gap-2 focus-visible:outline-none">
      {pairs.length === 0 && <p className="text-sm text-muted-foreground">{Array.isArray(value) ? t("form.noEntries") : t("form.notSet")}</p>}
      {pairs.map(([k, v], i) => (
        <div key={i} className="flex min-w-0 items-center gap-2">
          <Input aria-label={t("form.key", { n: i + 1 })} value={k} disabled={disabled} onBlur={onBlur} onChange={(e) => edit(i, 0, e.target.value)} className="min-w-0 flex-1" />
          <Input aria-label={t("form.value", { n: i + 1 })} value={v} disabled={disabled} onBlur={onBlur} onChange={(e) => edit(i, 1, e.target.value)} className="min-w-0 flex-[2]" />
          <Button size="icon" variant="ghost" aria-label={t("form.removeEntry", { n: i + 1 })} disabled={disabled} onClick={() => set(pairs.filter((_, j) => j !== i))}>
            <X />
          </Button>
        </div>
      ))}
      <div>
        <Button size="sm" variant="outline" disabled={disabled} onClick={() => set([...pairs, ["", ""]])}>
          <Plus /> {t("form.addEntry")}
        </Button>
        {nullable && Array.isArray(value) && (
          <Button size="sm" variant="ghost" className="ml-2" disabled={disabled} onClick={() => onChange("")}>
            {t("form.clear")}
          </Button>
        )}
      </div>
    </div>
  );
}
