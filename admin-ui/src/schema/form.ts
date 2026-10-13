// Form state ↔ wire values, and client-side rules (zod). The server re-validates
// everything; these rules only give earlier feedback.
import { z } from "zod";
import type { ComponentSchema, InputSchema, JsonValue } from "@/api/types";
import { asDocument } from "@/lib/rich-text";
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

/** A JSON object whose values are all text (what a key/value input edits). */
export const isTextObject = (value: unknown): value is Record<string, string> =>
  typeof value === "object" && value !== null && !Array.isArray(value) && Object.values(value).every((v) => typeof v === "string");

/** A stored key/value that is not an object of text: shown, never rewritten. */
const foreignKeyValue = (input: InputSchema, value: unknown) =>
  value !== null && value !== undefined &&
  ((input.type === "key_value" && !isTextObject(value)) || (input.type === "rich_editor" && !asDocument(value)));

/**
 * The form with JSON inputs made read-only where the stored value would lose
 * precision, or (key/value inputs) is not an object of text.
 */
export function protectLossyJson(components: ComponentSchema[], data?: Record<string, unknown>): ComponentSchema[] {
  if (!data) return components;
  return components.map((c) =>
    isInput(c)
      ? lossyJson(c, data[c.name])
        ? { ...c, disabled: true, help: "Holds numbers too large to edit here without changing them." }
        : foreignKeyValue(c, data[c.name])
          ? { ...c, disabled: true, help: "Holds values that are not text pairs: it cannot be edited here." }
          : c
      : { ...c, components: protectLossyJson(c.components, data) },
  );
}

/** Key/value pairs of a form value (each held as the JSON of `[key, value]`). */
export function pairsOf(value: FormValue): [string, string][] {
  return Array.isArray(value) ? value.map((p) => JSON.parse(p) as [string, string]) : [];
}

export const encodePair = (key: string, value: string) => JSON.stringify([key, value]);

/** Input types of vocabulary version 3. */
export const KNOWN_INPUT_TYPES = new Set<string>([
  "file_upload",
  "rich_editor",
  "key_value",
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
    case "file_upload":
      // One media id ("" when not set), or the ordered list of ids.
      if (input.media?.multiple) return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
      return typeof value === "string" ? value : "";
    case "rich_editor":
      // The document as JSON text ("" when not set); anything else is shown read-only.
      return value === null || value === undefined ? "" : JSON.stringify(value);
    case "key_value":
      // "" is null (not set); a list (possibly empty) is an object. Anything else
      // stays as its JSON text: shown read-only (`protectLossyJson`), never sent.
      if (value === null || value === undefined) return "";
      return isTextObject(value) ? Object.entries(value).map(([k, v]) => encodePair(k, v)) : JSON.stringify(value, null, 2);
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
    case "file_upload":
      if (input.media?.multiple) return Array.isArray(value) ? value : [];
      return value === "" ? null : value;
    case "rich_editor":
      return value === "" ? null : (JSON.parse(value as string) as JsonValue);
    case "key_value": {
      if (!Array.isArray(value)) return null;
      // No prototype: a key such as `__proto__` stays an ordinary key.
      const object = Object.create(null) as Record<string, string>;
      for (const [key, text] of pairsOf(value)) object[key] = text;
      return object;
    }
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
      case "file_upload": {
        const max = input.media?.maxItems ?? 1;
        shape[input.name] = z.union([z.string(), z.array(z.string())]).superRefine((value, ctx) => {
          if (input.disabled) return;
          const count = Array.isArray(value) ? value.length : value === "" ? 0 : 1;
          if (required && count === 0) ctx.addIssue({ code: "custom", message: "is required" });
          if (count > max) ctx.addIssue({ code: "custom", message: `may not have more than ${max} items` });
        });
        break;
      }
      case "rich_editor":
        shape[input.name] = z.string().superRefine((value, ctx) => {
          if (!input.disabled && required && value === "") ctx.addIssue({ code: "custom", message: "is required" });
        });
        break;
      case "key_value": {
        const items = input.rules.find((r) => r.rule === "max_items");
        const length = input.rules.find((r) => r.rule === "max_length");
        shape[input.name] = z.union([z.string(), z.array(z.string())]).superRefine((value, ctx) => {
          if (input.disabled) return;
          if (!Array.isArray(value)) {
            if (required) ctx.addIssue({ code: "custom", message: "is required" });
            return;
          }
          const pairs = pairsOf(value);
          const keys = pairs.map(([k]) => k);
          const chars = (text: string) => [...text].length;
          const issue = (message: string) => ctx.addIssue({ code: "custom", message });
          if (items && items.rule === "max_items" && pairs.length > items.value) issue(`may not have more than ${items.value} items`);
          else if (keys.some((k) => k.trim() === "")) issue("may not have an empty key");
          else if (new Set(keys).size !== keys.length) issue(`has the key "${keys.find((k, i) => keys.indexOf(k) !== i)}" twice`);
          else if (keys.some((k) => chars(k) > 255)) issue("may not have keys longer than 255 characters");
          else if (length && length.rule === "max_length" && pairs.some(([, v]) => chars(v) > length.value)) {
            issue(`may not have values longer than ${length.value} characters`);
          }
        });
        break;
      }
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
