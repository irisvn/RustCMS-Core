// Form state ↔ wire values, and client-side rules (zod). The server re-validates
// everything; these rules only give earlier feedback.
import { z } from "zod";
import type { ComponentSchema, InputSchema, JsonValue } from "@/api/types";
import { checkText, fromText, toText, type WireKind } from "@/lib/wire";

// No `new Function` (zod's JIT probes it): the panel's CSP forbids eval.
z.config({ jitless: true });

/** What a form holds per input: text, a flag, or several encoded values. */
export type FormValue = string | boolean | string[];
export type FormValues = Record<string, FormValue>;

/**
 * Whether a parsed JSON value holds integers JavaScript cannot represent
 * exactly: such a value was (or would be) rounded and must not be written back.
 */
export function hasUnsafeInteger(value: unknown): boolean {
  // Also non-finite numbers: `1e400` parses to Infinity and would be sent as null.
  if (typeof value === "number") return !Number.isFinite(value) || (Number.isInteger(value) && !Number.isSafeInteger(value));
  if (Array.isArray(value)) return value.some(hasUnsafeInteger);
  if (value && typeof value === "object") return Object.values(value).some(hasUnsafeInteger);
  return false;
}

/** JSON inputs whose stored value cannot be edited without losing precision. */
export const lossyJson = (input: InputSchema, value: unknown) =>
  (input.valueType as WireKind) === "json" && hasUnsafeInteger(value);

/** The form with JSON inputs made read-only where the stored value would lose precision. */
export function protectLossyJson(components: ComponentSchema[], data?: Record<string, unknown>): ComponentSchema[] {
  if (!data) return components;
  return components.map((c) =>
    isInput(c)
      ? lossyJson(c, data[c.name])
        ? { ...c, disabled: true, help: "Holds numbers too large to edit here without changing them." }
        : c
      : { ...c, components: protectLossyJson(c.components, data) },
  );
}

/** Input types of vocabulary version 1. */
export const KNOWN_INPUT_TYPES = new Set<string>([
  "text_input",
  "textarea",
  "number_input",
  "select",
  "toggle",
  "checkbox",
  "checkbox_list",
  "date_picker",
  "date_time_picker",
  "hidden",
]);

export const isInput = (c: ComponentSchema): c is InputSchema => "name" in c;

/** Every input of a form, depth-first. */
export function inputsOf(components: ComponentSchema[]): InputSchema[] {
  return components.flatMap((c) => (isInput(c) ? [c] : inputsOf(c.components)));
}

const kindOf = (input: InputSchema) => input.valueType as WireKind;
const encode = (v: unknown) => JSON.stringify(v ?? null);

/** Select values are kept as their JSON encoding so numbers and strings survive. */
/** Nullable booleans have three states (unset, no, yes), held as "", "false", "true". */
export const isTriState = (input: InputSchema) =>
  (input.type === "toggle" || input.type === "checkbox") && input.nullable;

export function toFormValue(input: InputSchema, value: unknown): FormValue {
  if (isTriState(input)) return value === null || value === undefined ? "" : value ? "true" : "false";
  switch (input.type) {
    case "toggle":
    case "checkbox":
      return value === true;
    case "checkbox_list":
      return Array.isArray(value) ? value.map(encode) : [];
    case "select":
      return value === null || value === undefined ? "" : encode(value);
    default:
      if (input.writeOnly) return "";
      if (kindOf(input) === "json") return value === null || value === undefined ? "" : JSON.stringify(value, null, 2);
      return toText(kindOf(input), value);
  }
}

const TEXT_KINDS = new Set<WireKind>(["string"]);

export function fromFormValue(input: InputSchema, value: FormValue): unknown {
  if (isTriState(input)) return value === "" ? null : value === "true";
  switch (input.type) {
    case "toggle":
    case "checkbox":
      return value === true;
    case "checkbox_list":
      return (value as string[]).map((v) => JSON.parse(v) as JsonValue);
    case "select":
      return value === "" ? null : (JSON.parse(value as string) as JsonValue);
    default: {
      const text = value as string;
      if (text === "") {
        // Emptied: null where the column allows it, "" for text that does not
        // (other kinds send null and the server answers "is required").
        return input.nullable || !TEXT_KINDS.has(kindOf(input)) ? null : "";
      }
      if (kindOf(input) === "json") return JSON.parse(text) as JsonValue;
      return fromText(kindOf(input), text);
    }
  }
}

/** Form values from a record, or for a new record from the schema's declared defaults. */
export function defaultValues(inputs: InputSchema[], data?: Record<string, unknown>): FormValues {
  return Object.fromEntries(inputs.map((i) => [i.name, toFormValue(i, data ? data[i.name] : i.default)]));
}

/**
 * Payload of a submit: enabled inputs only; on edit only changed ones
 * (`dirty`), so fields changed by someone else meanwhile are not overwritten.
 * Write-only inputs are sent only when filled in.
 */
export function payloadOf(
  inputs: InputSchema[],
  values: FormValues,
  dirty?: Partial<Record<string, unknown>>,
): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const input of inputs) {
    // Disabled inputs are not writable; unknown (newer) input types are never submitted.
    if (input.disabled || !KNOWN_INPUT_TYPES.has(input.type)) continue;
    if (dirty && !dirty[input.name]) continue;
    const value = values[input.name];
    if (value === undefined) continue;
    if (input.writeOnly && value === "") continue;
    // Hidden inputs only send back a value they were given; an unset one is left
    // to server defaults and hooks.
    if (input.type === "hidden" && value === "") continue;
    out[input.name] = fromFormValue(input, value);
  }
  return out;
}

export function zodSchema(inputs: InputSchema[]) {
  const shape: Record<string, z.ZodType> = {};
  for (const input of inputs) {
    const required = input.rules.some((r) => r.rule === "required") && !input.writeOnly;
    const max = input.rules.find((r) => r.rule === "max_length");
    if (isTriState(input)) {
      shape[input.name] = z.string().refine((v) => !(required && v === "" && !input.disabled), "is required");
      continue;
    }
    switch (input.type) {
      case "toggle":
      case "checkbox":
        shape[input.name] = z.boolean();
        break;
      case "checkbox_list":
        shape[input.name] = z
          .array(z.string())
          .refine((values) => !values.some((v) => hasUnsafeInteger(JSON.parse(v))), "has a value that cannot be kept exactly");
        break;
      default:
        shape[input.name] = z.string().superRefine((text, ctx) => {
          // Hidden inputs cannot be corrected by the user (nor show an error): the server decides.
          if (input.disabled || input.type === "hidden") return;
          if (text === "") {
            if (required) ctx.addIssue({ code: "custom", message: "is required" });
            return;
          }
          // Selects hold JSON-encoded values: measure the value itself.
          const decoded = input.type === "select" ? (JSON.parse(text) as unknown) : text;
          if (max && max.rule === "max_length" && typeof decoded === "string" && [...decoded].length > max.value) {
            ctx.addIssue({ code: "custom", message: `may not be longer than ${max.value} characters` });
          }
          if (input.type === "select") {
            if (hasUnsafeInteger(decoded)) ctx.addIssue({ code: "custom", message: "has a value that cannot be kept exactly" });
            return;
          }
          if (kindOf(input) === "json") {
            try {
              if (hasUnsafeInteger(JSON.parse(text))) {
                ctx.addIssue({ code: "custom", message: "has numbers that cannot be kept exactly; store them as strings" });
              }
            } catch {
              ctx.addIssue({ code: "custom", message: "must be valid JSON" });
            }
            return;
          }
          const error = checkText(kindOf(input), text);
          if (error) ctx.addIssue({ code: "custom", message: error });
        });
    }
  }
  return z.object(shape);
}
