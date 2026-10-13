// Renders a form schema: layouts and inputs from the vocabulary. Unknown types
// fall back to a read-only rendering instead of breaking the page.
import { Controller, type Control, type FieldErrors } from "react-hook-form";
import { Plus, X } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import type * as React from "react";
import type { ComponentSchema, Data, InputSchema, LayoutSchema, SelectOption } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Checkbox, Switch } from "@/components/ui/checkbox";
import { Input, Label, NativeSelect, Textarea } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import {
  KNOWN_INPUT_TYPES,
  encodePair,
  hasUnsafeInteger,
  inputsOf,
  isInput,
  isTextObject,
  isTriState,
  pairsOf,
  type FormValue,
  type FormValues,
} from "@/schema/form";
import { RelationSelect, RelationValue } from "./relation-select";
import { FileUploadWidget, MediaThumb } from "./media";
import { RichEditorWidget, RichTextView } from "./rich-text";
import { asDocument } from "@/lib/rich-text";

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
}

export const KNOWN_INPUTS = KNOWN_INPUT_TYPES;
export const KNOWN_LAYOUTS = new Set(["section", "grid", "tabs", "tab"]);

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
const span = (n: number) => SPAN[Math.min(Math.max(n, 1), 4)] ?? SPAN[1];

export function Components({ components, ctx }: { components: ComponentSchema[]; ctx: FormContext }) {
  return (
    <>
      {components.map((c, i) =>
        isInput(c) ? <Field key={c.name} input={c} ctx={ctx} /> : <Layout key={`layout-${i}`} layout={c} ctx={ctx} />,
      )}
    </>
  );
}

function SchemaLayout({ layout, children }: { layout: LayoutSchema; children: React.ReactNode }) {
  const grid = <div className={cn("grid grid-cols-1 gap-x-6 gap-y-6", gridCols(layout.columns))}>{children}</div>;
  if (layout.type === "section" || !KNOWN_LAYOUTS.has(layout.type)) {
    return (
      <fieldset className="col-span-full min-w-0 rounded-lg border border-border p-5 sm:p-6">
        {layout.label && <legend className="px-2 text-sm font-semibold tracking-tight">{layout.label}</legend>}
        {grid}
      </fieldset>
    );
  }
  return <div className="col-span-full min-w-0">{grid}</div>;
}

function Layout({ layout, ctx }: { layout: LayoutSchema; ctx: FormContext }) {
  if (layout.type === "tabs") {
    return <TabsLayout layout={layout} errors={ctx.errors} attempt={ctx.submitCount} render={(tab) => <Components components={tab.components} ctx={ctx} />} />;
  }
  return <SchemaLayout layout={layout}><Components components={layout.components} ctx={ctx} /></SchemaLayout>;
}

/**
 * WAI-ARIA tabs. Every panel stays mounted (drafts survive switching). After
 * each failed save (and when server errors arrive), the tab holding the first
 * error is shown; nested tabs do the same, so every enclosing tab opens. The
 * form then focuses that input.
 */
function TabsLayout({ layout, errors, attempt, render }: {
  layout: LayoutSchema;
  errors?: FieldErrors<FormValues>;
  attempt?: number;
  render: (tab: LayoutSchema) => React.ReactNode;
}) {
  const base = useId();
  const tabs = layout.components.filter((c): c is LayoutSchema => !isInput(c));
  const [active, setActive] = useState(0);
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const current = Math.min(active, Math.max(tabs.length - 1, 0));
  const failing = tabs.map((tab) => inputsOf(tab.components).filter((i) => errors?.[i.name]).map((i) => i.name));
  const signature = failing.map((names) => names.join(",")).join("|");
  useEffect(() => {
    const index = failing.findIndex((names) => names.length > 0);
    if (index >= 0) setActive(index);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature, attempt]);
  const select = (index: number) => {
    const next = (index + tabs.length) % tabs.length;
    setActive(next);
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
      <div role="tablist" aria-label={layout.label ?? "Sections"} className="flex flex-wrap gap-1 border-b border-border" onKeyDown={onKey}>
        {tabs.map((tab, i) => {
          const id = tab.id ?? String(i);
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
              onClick={() => setActive(i)}
              className={cn(
                "-mb-px inline-flex min-h-9 items-center gap-2 border-b-2 px-3 text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                i === current ? "border-foreground text-foreground" : "border-transparent text-muted-foreground hover:text-foreground",
              )}
            >
              {tab.label ?? id}
              {count > 0 && (
                <span className="rounded-full bg-destructive px-1.5 text-xs text-destructive-foreground">
                  {count}
                  <span className="sr-only"> {count === 1 ? "error" : "errors"}</span>
                </span>
              )}
            </button>
          );
        })}
      </div>
      {tabs.map((tab, i) => {
        const id = tab.id ?? String(i);
        return (
          <div key={id} role="tabpanel" id={`${base}-panel-${id}`} aria-labelledby={`${base}-tab-${id}`} hidden={i !== current} className="pt-5">
            <div className={cn("grid grid-cols-1 gap-x-6 gap-y-6", gridCols(tab.columns))}>{render(tab)}</div>
          </div>
        );
      })}
    </div>
  );
}

/** Read only the supplied view schema and record. Defaults are never stored values. */
export function ReadOnlyComponents({ components, data, slug, record }: {
  components: ComponentSchema[];
  data: Data;
  slug: string;
  record: string;
}) {
  return <>{components.map((component, index) => {
    if (!isInput(component) && component.type === "tabs") {
      return <TabsLayout key={`layout-${index}`} layout={component} render={(tab) =>
        <ReadOnlyComponents components={tab.components} data={data} slug={slug} record={record} />} />;
    }
    if (!isInput(component)) {
      return <SchemaLayout key={`layout-${index}`} layout={component}>
        <ReadOnlyComponents components={component.components} data={data} slug={slug} record={record} />
      </SchemaLayout>;
    }
    if (component.type === "hidden" || component.writeOnly) return null;
    const supplied = Object.prototype.hasOwnProperty.call(data, component.name);
    const value = supplied ? data[component.name] : undefined;
    const unknown = !KNOWN_INPUTS.has(component.type);
    let content: React.ReactNode;
    if (!supplied || value === undefined) content = <span className="text-muted-foreground">Not provided</span>;
    else if (value === null) content = <span className="text-muted-foreground">Not set (null)</span>;
    else if (value === "") content = <span className="text-muted-foreground">Empty string</span>;
    else if (component.type === "select" && component.relationship &&
      (typeof value === "string" || (typeof value === "number" && Number.isSafeInteger(value)))) {
      content = <RelationValue slug={slug} field={component.name} record={record} value={JSON.stringify(value)} />;
    } else if (component.type === "select") {
      // Option labels first: a select may offer booleans with their own labels.
      const option = component.options?.find((option) => JSON.stringify(option.value) === JSON.stringify(value));
      content = option?.label ?? (typeof value === "boolean" ? (value ? "Yes" : "No") : readOnlyText(value));
    } else if (component.type === "file_upload" && (typeof value === "string" || Array.isArray(value))) {
      const ids = (Array.isArray(value) ? value : [value]).filter((v): v is string => typeof v === "string");
      content = <div className="flex flex-wrap gap-3">{ids.map((id) => <MediaThumb key={id} id={id} />)}</div>;
    } else if (component.type === "rich_editor" && asDocument(value)) {
      content = <RichTextView doc={asDocument(value)!} />;
    } else if (component.type === "key_value" && isTextObject(value)) {
      const entries = Object.entries(value);
      content = entries.length === 0 ? <span className="text-muted-foreground">No entries</span> : (
        <dl className="grid grid-cols-[minmax(0,auto)_minmax(0,1fr)] gap-x-4 gap-y-1">
          {entries.map(([k, v]) => [
            <dt key={`k-${k}`} className="font-medium">{k}</dt>,
            <dd key={`v-${k}`} className="break-words">{v}</dd>,
          ])}
        </dl>
      );
    } else if (typeof value === "boolean") content = value ? "Yes" : "No";
    else if (component.type === "text_input" && component.mode === "password") {
      // A readable password-mode field is still not shown in clear.
      content = <span aria-label="Hidden value">••••••••</span>;
    } else if (component.type === "checkbox_list" && Array.isArray(value)) {
      content = value.length === 0 ? "Empty list" : <ul className="space-y-1">{value.map((item, i) =>
        <li key={i}>{component.options?.find((option) => JSON.stringify(option.value) === JSON.stringify(item))?.label ?? readOnlyText(item)}</li>,
      )}</ul>;
    } else content = readOnlyText(value);
    return <dl key={component.name} className={cn("min-w-0 space-y-2", span(component.columnSpan))}>
      <dt className="text-sm font-medium text-muted-foreground">{component.label}</dt>
      <dd className="select-text whitespace-pre-wrap break-words text-sm leading-6" data-fallback={unknown || undefined}>{content}</dd>
    </dl>;
  })}</>;
}

function readOnlyText(value: unknown): string {
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

interface WidgetProps {
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

function Widget({ id, input, ctx, value, onChange, onBlur, disabled, invalid, describedBy }: WidgetProps) {
  if (isTriState(input)) {
    // Nullable boolean: "not set" is a value of its own, and can be restored.
    return (
      <NativeSelect id={id} disabled={disabled} onBlur={onBlur} aria-invalid={invalid || undefined} aria-describedby={describedBy} value={typeof value === "string" ? value : ""} onChange={(e) => onChange(e.target.value)}>
        <option value="">Not set</option>
        <option value="true">Yes</option>
        <option value="false">No</option>
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
      return (
        <Input
          {...common}
          type={type}
          value={text}
          onChange={onText}
          autoComplete={input.mode === "password" ? "new-password" : undefined}
          placeholder={input.writeOnly ? "Leave empty to keep the current value" : input.placeholder}
        />
      );
    }
    case "textarea":
      return <Textarea {...common} value={text} onChange={onText} rows={4} />;
    case "number_input":
      // Text, not type=number: i64/decimal must stay exact strings.
      return <Input {...common} inputMode="decimal" value={text} onChange={onText} />;
    case "date_picker":
      return <Input {...common} type="date" value={text} onChange={onText} />;
    case "date_time_picker":
      if (input.valueType === "instant") {
        return (
          <div className="flex items-center gap-2">
            <Input {...common} type="datetime-local" step={1} value={text} onChange={onText} />
            <span className="text-xs text-muted-foreground">UTC</span>
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
          {(input.options ?? []).map((o) => (
            <option key={encodeOption(o)} value={encodeOption(o)} disabled={hasUnsafeInteger(o.value)}>
              {o.label}
            </option>
          ))}
        </NativeSelect>
      );
    case "file_upload":
      return <FileUploadWidget id={id} input={input} value={value} onChange={onChange} disabled={disabled} invalid={invalid} describedBy={describedBy} onBusy={(busy) => ctx.onBusy?.(input.name, busy)} />;
    case "rich_editor":
      return <RichEditorWidget id={id} label={input.label} value={text} onChange={onChange} disabled={disabled} invalid={invalid} describedBy={describedBy} nullable={input.nullable} onBusy={(busy) => ctx.onBusy?.(input.name, busy)} />;
    case "key_value":
      return <KeyValueEditor id={id} label={input.label} nullable={input.nullable} value={value} onChange={onChange} onBlur={onBlur} disabled={disabled} invalid={invalid} describedBy={describedBy} />;
    default:
      // Unknown component (newer vocabulary): show the value, never edit it.
      return <Input {...common} value={Array.isArray(value) ? value.join(", ") : String(value)} disabled readOnly data-fallback="true" />;
  }
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
      {pairs.length === 0 && <p className="text-sm text-muted-foreground">{Array.isArray(value) ? "No entries." : "Not set."}</p>}
      {pairs.map(([k, v], i) => (
        <div key={i} className="flex min-w-0 items-center gap-2">
          <Input aria-label={`Key ${i + 1}`} value={k} disabled={disabled} onBlur={onBlur} onChange={(e) => edit(i, 0, e.target.value)} className="min-w-0 flex-1" />
          <Input aria-label={`Value ${i + 1}`} value={v} disabled={disabled} onBlur={onBlur} onChange={(e) => edit(i, 1, e.target.value)} className="min-w-0 flex-[2]" />
          <Button size="icon" variant="ghost" aria-label={`Remove entry ${i + 1}`} disabled={disabled} onClick={() => set(pairs.filter((_, j) => j !== i))}>
            <X />
          </Button>
        </div>
      ))}
      <div>
        <Button size="sm" variant="outline" disabled={disabled} onClick={() => set([...pairs, ["", ""]])}>
          <Plus /> Add entry
        </Button>
        {nullable && Array.isArray(value) && (
          <Button size="sm" variant="ghost" className="ml-2" disabled={disabled} onClick={() => onChange("")}>
            Clear (not set)
          </Button>
        )}
      </div>
    </div>
  );
}
