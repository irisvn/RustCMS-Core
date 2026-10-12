// Renders a form schema: layouts and inputs from the vocabulary. Unknown types
// fall back to a read-only rendering instead of breaking the page.
import { Controller, type Control, type FieldErrors } from "react-hook-form";
import type * as React from "react";
import type { ComponentSchema, InputSchema, LayoutSchema, SelectOption } from "@/api/types";
import { Checkbox, Switch } from "@/components/ui/checkbox";
import { Input, Label, NativeSelect, Textarea } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { KNOWN_INPUT_TYPES, hasUnsafeInteger, isInput, isTriState, type FormValue, type FormValues } from "@/schema/form";
import { RelationSelect } from "./relation-select";

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
}

export const KNOWN_INPUTS = KNOWN_INPUT_TYPES;
export const KNOWN_LAYOUTS = new Set(["section", "grid"]);

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

function Layout({ layout, ctx }: { layout: LayoutSchema; ctx: FormContext }) {
  const grid = <div className={cn("grid grid-cols-1 gap-4", gridCols(layout.columns))}><Components components={layout.components} ctx={ctx} /></div>;
  if (layout.type === "section" || !KNOWN_LAYOUTS.has(layout.type)) {
    return (
      <fieldset className="col-span-full rounded-lg border border-border p-4">
        {layout.label && <legend className="px-1 text-sm font-semibold">{layout.label}</legend>}
        {grid}
      </fieldset>
    );
  }
  return <div className="col-span-full">{grid}</div>;
}

function Field({ input, ctx }: { input: InputSchema; ctx: FormContext }) {
  if (input.type === "hidden") return null;
  const id = `field-${input.name}`;
  const error = ctx.errors[input.name]?.message;
  const disabled = input.disabled || ctx.readOnly;
  const required = input.rules.some((r) => r.rule === "required");
  return (
    <div className={cn("flex flex-col gap-1.5", span(input.columnSpan))}>
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
          />
        )}
      />
      {input.help && <p className="text-xs text-muted-foreground">{input.help}</p>}
      {error && (
        <p role="alert" className="text-xs text-destructive">
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
}

const encodeOption = (o: SelectOption) => JSON.stringify(o.value);

function Widget({ id, input, ctx, value, onChange, onBlur, disabled, invalid }: WidgetProps) {
  if (isTriState(input)) {
    // Nullable boolean: "not set" is a value of its own, and can be restored.
    return (
      <NativeSelect id={id} disabled={disabled} onBlur={onBlur} aria-invalid={invalid || undefined} value={typeof value === "string" ? value : ""} onChange={(e) => onChange(e.target.value)}>
        <option value="">Not set</option>
        <option value="true">Yes</option>
        <option value="false">No</option>
      </NativeSelect>
    );
  }
  const common = { id, disabled, onBlur, "aria-invalid": invalid || undefined, placeholder: input.placeholder };
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
      return <Switch id={id} checked={value === true} onCheckedChange={onChange} disabled={disabled} aria-label={input.label} />;
    case "checkbox":
      return (
        <label className="flex items-center gap-2 text-sm font-medium">
          <Checkbox id={id} checked={value === true} onChange={(e) => onChange(e.target.checked)} disabled={disabled} />
          {input.label}
        </label>
      );
    case "checkbox_list": {
      const selected = Array.isArray(value) ? value : [];
      return (
        <div id={id} role="group" aria-label={input.label} className="flex flex-col gap-1.5">
          {(input.options ?? []).map((o) => {
            const encoded = encodeOption(o);
            return (
              <label key={encoded} className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={selected.includes(encoded)}
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
    default:
      // Unknown component (newer vocabulary): show the value, never edit it.
      return <Input id={id} value={Array.isArray(value) ? value.join(", ") : String(value)} disabled readOnly data-fallback="true" />;
  }
}
