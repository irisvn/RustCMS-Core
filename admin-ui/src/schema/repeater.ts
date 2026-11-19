// JSON repeaters (ADR 0040): the rows of a list, held in the form as JSON
// text of each row's form values (what the inputs show, typing included),
// turned into wire values when sent, and checked in the browser with the
// rules each row input declares. The server checks them again and decides
// (`crates/rustcms-admin/src/engine/repeater.rs`).
import type { InputSchema, JsonValue, RepeaterSchema } from "@/api/types";
import { t } from "@/i18n";
import { type Level, type Outer, formConditions, hasConditions, hiddenFields, readsOutside } from "./conditions";
import { type FormValue, type FormValues, fromFormValue, inputsOf, toFormValue, wireValues, zodSchema } from "./form";

/** A row's form values, by input name. */
export type Row = FormValues;

/**
 * A row of wire values as form values (missing inputs take their empty form
 * value). A new row's nested repeaters start with their declared rows; a
 * stored row's missing ones hold none.
 */
export function rowOf(repeater: RepeaterSchema, wire: Record<string, unknown>, stored = false): Row {
  return Object.fromEntries(
    inputsOf(repeater.components).map((i) => [i.name, toFormValue(i, stored && wire[i.name] === undefined && i.type === "repeater" ? null : wire[i.name])]),
  );
}

/** The form value of a repeater holding `wire` rows (undefined: a new record's empty rows). */
export function repeaterFormValue(input: InputSchema, wire: unknown): FormValue {
  const repeater = input.repeater;
  if (!repeater) return "";
  if (wire === undefined) return JSON.stringify(Array.from({ length: repeater.defaultItems ?? 0 }, () => rowOf(repeater, {})));
  if (!Array.isArray(wire)) return wire === null ? "" : JSON.stringify(wire);
  return JSON.stringify(wire.map((row) => rowOf(repeater, row && typeof row === "object" ? (row as Record<string, unknown>) : {}, true)));
}

/** The rows a form value holds (`""`: none), or null when it is not a list of rows. */
export function parseRows(value: FormValue): Row[] | null {
  if (typeof value !== "string" || value === "") return [];
  try {
    const rows = JSON.parse(value) as unknown;
    return Array.isArray(rows) && rows.every((r) => r !== null && typeof r === "object" && !Array.isArray(r)) ? (rows as Row[]) : null;
  } catch {
    return null;
  }
}

/** The wire rows of a repeater's form value (empty inputs left out). */
export function wireRows(input: InputSchema, value: FormValue): JsonValue {
  const repeater = input.repeater;
  const rows = parseRows(value);
  if (!repeater || rows === null) return null;
  const inputs = inputsOf(repeater.components);
  return rows.map((row) => {
    const out: Record<string, JsonValue> = {};
    for (const i of inputs) {
      const form = row[i.name];
      if (form === undefined) continue;
      const wire = fromFormValue(i, form) as JsonValue | undefined;
      if (wire === null || wire === undefined || wire === "") continue;
      out[i.name] = wire;
    }
    return out;
  });
}

/**
 * The inputs of `row` hidden by their conditions: they name inputs of the
 * same row, or of the levels of `outer` (`when_parent`, `when_root`).
 */
export function hiddenInRow(repeater: RepeaterSchema, row: Row, outer: Outer = {}): Set<string> {
  return rowHider(repeater, outer)(row);
}

/**
 * [`hiddenInRow`] for many rows of `repeater`, its conditions read once.
 * Without `outer` (not known where rows are checked before a save), inputs
 * whose conditions read outside the row count as hidden: the server decides
 * on them.
 */
function rowHider(repeater: RepeaterSchema, outer?: Outer): (row: Row) => Set<string> {
  if (!hasConditions(repeater.components)) return () => new Set();
  const conditions = formConditions(repeater.components);
  const inputs = inputsOf(repeater.components);
  const unknown = outer
    ? []
    : conditions.fields.filter((f) => f.conditions.some(readsOutside)).map((f) => f.name);
  return (row) => {
    const hidden = hiddenFields(conditions, wireValues(inputs, row), outer ?? {});
    unknown.forEach((name) => hidden.add(name));
    return hidden;
  };
}

/** A row as the rows nested in it read it (`when_parent`): wire values, hidden inputs null. */
export function rowLevel(repeater: RepeaterSchema, row: Row, hidden: Set<string>): Level {
  const inputs = inputsOf(repeater.components);
  const values = wireValues(inputs, row);
  hidden.forEach((name) => (values[name] = null));
  return { values, kinds: new Map(inputs.map((i) => [i.name, i.valueType])) };
}

/**
 * What is wrong with `rows`, by path below the repeater: `""` for the list,
 * `"2.question"` for an input, `"2.links.0.url"` nested. Hidden inputs are
 * not checked (never stored); without `outer`, neither are inputs whose
 * conditions read outside their row.
 */
export function rowProblems(repeater: RepeaterSchema, rows: Row[], prefix = "", outer?: Outer): Record<string, string> {
  const out: Record<string, string> = {};
  const at = (path: string) => (prefix ? (path ? `${prefix}.${path}` : prefix) : path);
  if (repeater.minItems !== undefined && rows.length < repeater.minItems) out[at("")] = t("issue.min_items", { min: repeater.minItems });
  if (repeater.maxItems !== undefined && rows.length > repeater.maxItems) out[at("")] = t("issue.max_items", { max: repeater.maxItems });
  const inputs = inputsOf(repeater.components);
  // Built once per list, not per row.
  const schemas = new Map(inputs.filter((i) => i.type !== "repeater").map((i) => [i.name, zodSchema([i])]));
  const hiddenIn = rowHider(repeater, outer);
  rows.forEach((row, index) => {
    const hidden = hiddenIn(row);
    const below = outer ? { parent: rowLevel(repeater, row, hidden), root: outer.root } : undefined;
    for (const input of inputs) {
      if (hidden.has(input.name)) continue;
      const path = at(`${index}.${input.name}`);
      const value = row[input.name] ?? toFormValue(input, undefined);
      // A relationship repeater's rows are checked by `relationRowProblems` (ADR 0044, part C).
      if (input.repeater?.relationship) continue;
      if (input.type === "repeater" && input.repeater) {
        const nested = parseRows(value) ?? [];
        Object.assign(out, rowProblems(input.repeater, nested, path, below));
        continue;
      }
      const result = schemas.get(input.name)!.safeParse({ [input.name]: value });
      if (!result.success) {
        out[path] = result.error.issues[0]?.message ?? t("issue.invalid");
      } else if (input.integer && typeof value === "string" && value !== "" && !Number.isInteger(Number(value))) {
        out[path] = t("issue.integer");
      }
    }
  });
  return out;
}

/** A row's heading from the repeater's `itemLabel` template (`{question}`), or "Item n". */
export function itemLabel(repeater: RepeaterSchema, row: Row, index: number): string {
  const fallback = t("repeater.item", { n: index + 1 });
  if (!repeater.itemLabel) return fallback;
  const inputs = inputsOf(repeater.components);
  const text = repeater.itemLabel.replace(/\{([a-z0-9_]+)\}/gi, (_, name: string) => {
    const input = inputs.find((i) => i.name === name);
    const value = row[name];
    if (!input || typeof value !== "string") return "";
    if (input.type === "select") {
      // Selects hold JSON: show the option's label.
      const option = input.options?.find((o) => JSON.stringify(o.value) === value);
      return option?.label ?? "";
    }
    return value;
  });
  const trimmed = text.replace(/\s+/g, " ").trim();
  return trimmed || fallback;
}
