// Form reactions (ADR 0043): what the form sends to the server's reaction,
// and the changes it answers with applied to the form's values. The server
// decides; these only help the user while typing (the save is checked again).
import type { FormPatch, InputSchema, JsonValue } from "@/api/types";
import { isResourceSecret } from "./secrets";
import { type FormValue, type FormValues, fromFormValue, inputsOf, toFormValue } from "./form";
import { type RelationState, createView, newRef, parseState, rowWire } from "./relation-rows";
import { type Row, parseRows, rowOf } from "./repeater";

/** The rows of a relationship repeater as the form holds them before any change (the stored rows read), if known. */
export type StoredRows = (input: InputSchema) => RelationState | null;

/** A relationship repeater's rows as the form holds them: its changes, else the stored rows. */
function relationState(input: InputSchema, value: FormValue | undefined, stored?: StoredRows): RelationState | null {
  return parseState(value ?? "") ?? stored?.(input) ?? null;
}

/** Pause in typing before a reaction is asked for. */
export const REACTION_DELAY_MS = 400;

/** Inputs whose values a reaction never receives: secrets, links, child records, write-only ones. */
const withheld = (input: InputSchema) =>
  isResourceSecret(input) ||
  input.type === "secret" ||
  input.type === "secret_map" ||
  input.type === "belongs_to_many" ||
  (input.type === "repeater" && !!input.repeater?.relationship) ||
  !!input.writeOnly;

/**
 * The form's values in wire format, as a reaction receives them (a value that
 * does not convert is left out); a relationship repeater's rows as
 * `[{key | ref, data}]` (ADR 0044, part D), as the form holds them.
 */
export function reactionState(inputs: InputSchema[], values: FormValues, stored?: StoredRows): Record<string, JsonValue> {
  const out: Record<string, JsonValue> = {};
  for (const input of inputs) {
    const value = values[input.name];
    if (input.type === "repeater" && input.repeater?.relationship && !input.disabled) {
      const state = relationState(input, value, stored);
      if (state) {
        const repeater = input.repeater;
        out[input.name] = state.rows.map((row) => ({ ...(row.key ? { key: row.key } : { ref: row.ref ?? "" }), data: rowWire(repeater, row) }));
      }
      continue;
    }
    if (value === undefined || withheld(input)) continue;
    try {
      const wire = fromFormValue(input, value);
      if (wire !== undefined) out[input.name] = wire as JsonValue;
    } catch {
      // Half typed (a JSON text, a number): the server would drop it anyway.
    }
  }
  return out;
}

/** Warnings under rows, by repeater and row index. */
export type RowFlags = Record<string, Record<number, string[]>>;

export interface Applied {
  /** New form values, by input. */
  values: Record<string, FormValue>;
  flags: RowFlags;
  notices: string[];
}

const empty = (value: unknown) =>
  value === null || value === undefined || value === "" || (Array.isArray(value) && value.length === 0);

/**
 * `patches` applied in order to `values` (each sees the previous ones' result).
 * A patch naming an input the form does not have, or rows of an input that is
 * not a JSON repeater, is skipped: the server already dropped what the user
 * could not do. A repeater's rows are parsed once and written once; a row's
 * warning stays with that row when later patches move rows.
 */
export function applyPatches(inputs: InputSchema[], values: FormValues, patches: FormPatch[], stored?: StoredRows): Applied {
  const byName = new Map(inputs.map((i) => [i.name, i]));
  const out: Applied = { values: {}, flags: {}, notices: [] };
  const lists = new Map<string, { rows: Row[]; flags: Map<Row, string[]>; changed: boolean }>();
  const current = (name: string): FormValue | undefined => {
    const list = lists.get(name);
    if (list?.changed) return JSON.stringify(list.rows);
    return name in out.values ? out.values[name] : values[name];
  };
  const listOf = (name: string) => {
    let list = lists.get(name);
    if (!list) {
      const rows = parseRows(current(name) ?? "");
      if (rows === null) return null;
      list = { rows, flags: new Map(), changed: false };
      lists.set(name, list);
    }
    return list;
  };
  // Relationship repeaters' rows (ADR 0044, part D), changed in place and written once.
  const relations = new Map<string, { state: RelationState; flags: Map<string, string[]>; changed: boolean }>();
  const relationOf = (input: InputSchema) => {
    let entry = relations.get(input.name);
    if (!entry) {
      const state = relationState(input, values[input.name], stored);
      if (!state) return null;
      entry = { state: { ...state, rows: [...state.rows], deleted: [...state.deleted] }, flags: new Map(), changed: false };
      relations.set(input.name, entry);
    }
    return entry;
  };
  for (const patch of patches) {
    if (patch.op === "notice") {
      out.notices.push(patch.message);
      continue;
    }
    if (patch.op === "add_relation_row" || patch.op === "update_relation_row" || patch.op === "remove_relation_row" || patch.op === "flag_relation_row") {
      const input = byName.get(patch.field);
      const repeater = input?.repeater;
      if (!input || input.disabled || !repeater?.relationship) continue;
      const entry = relationOf(input);
      if (!entry) continue;
      const rows = entry.state.rows;
      if (patch.op === "add_relation_row") {
        const ref = patch.ref || newRef();
        rows.push({ ref, values: rowOf(createView(repeater), patch.data as Record<string, unknown>, true), changed: [] });
        entry.changed = true;
        continue;
      }
      const at = rows.findIndex((r) => (r.key ?? r.ref) === patch.id);
      const row = rows[at];
      if (!row) continue;
      if (patch.op === "update_relation_row") {
        // A stored row the user may not change is left as it is.
        if (row.key && row.update === false) continue;
        const view = row.key ? repeater : createView(repeater);
        const byInput = new Map(inputsOf(view.components).map((i) => [i.name, i]));
        const next = { ...row.values };
        const changed = new Set(row.changed);
        for (const [name, wire] of Object.entries(patch.data)) {
          const rowInput = byInput.get(name);
          if (!rowInput) continue;
          next[name] = toFormValue(rowInput, wire);
          if (row.key) changed.add(name);
        }
        rows[at] = { ...row, values: next, changed: [...changed] };
        entry.changed = true;
      } else if (patch.op === "remove_relation_row") {
        if (row.key && row.delete === false) continue;
        rows.splice(at, 1);
        if (row.key) entry.state.deleted.push(row.key);
        entry.flags.delete(patch.id);
        entry.changed = true;
      } else {
        entry.flags.set(patch.id, [...(entry.flags.get(patch.id) ?? []), patch.message]);
      }
      continue;
    }
    const input = byName.get(patch.field);
    if (!input || input.disabled || withheld(input)) continue;
    if (patch.op === "set" || patch.op === "set_if_empty") {
      if (patch.op === "set_if_empty") {
        const value = current(input.name);
        let wire: unknown = null;
        try {
          wire = value === undefined ? null : fromFormValue(input, value);
        } catch {
          wire = value;
        }
        if (!empty(wire)) continue;
      }
      out.values[input.name] = toFormValue(input, patch.value);
      // New rows: earlier row changes and warnings are replaced.
      lists.delete(input.name);
      continue;
    }
    const repeater = input.repeater;
    if (input.type !== "repeater" || !repeater) continue;
    const list = listOf(input.name);
    if (!list) continue;
    const rows = list.rows;
    switch (patch.op) {
      case "add_row": {
        const at = patch.index ?? rows.length;
        if (at > rows.length) continue;
        const row = patch.row && typeof patch.row === "object" && !Array.isArray(patch.row) ? (patch.row as Record<string, unknown>) : {};
        rows.splice(at, 0, rowOf(repeater, row, true));
        list.changed = true;
        break;
      }
      case "remove_row": {
        if (patch.index >= rows.length) continue;
        const [gone] = rows.splice(patch.index, 1);
        if (gone) list.flags.delete(gone);
        list.changed = true;
        break;
      }
      case "move_row":
        if (patch.from >= rows.length || patch.to >= rows.length) continue;
        rows.splice(patch.to, 0, rows.splice(patch.from, 1)[0]!);
        list.changed = true;
        break;
      case "flag_row": {
        const row = rows[patch.index];
        if (!row) continue;
        const messages = list.flags.get(row) ?? [];
        messages.push(patch.message);
        list.flags.set(row, messages);
        break;
      }
    }
  }
  for (const [name, entry] of relations) {
    if (entry.changed) out.values[name] = JSON.stringify(entry.state);
    const flags: Record<number, string[]> = {};
    entry.state.rows.forEach((row, index) => {
      const messages = entry.flags.get(row.key ?? row.ref ?? "");
      if (messages) flags[index] = messages;
    });
    if (Object.keys(flags).length > 0) out.flags[name] = flags;
  }
  for (const [name, list] of lists) {
    if (list.changed) out.values[name] = JSON.stringify(list.rows);
    const flags: Record<number, string[]> = {};
    list.flags.forEach((messages, row) => {
      const index = list.rows.indexOf(row);
      if (index >= 0) flags[index] = messages;
    });
    if (Object.keys(flags).length > 0) out.flags[name] = flags;
  }
  return out;
}
