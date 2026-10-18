// Encrypted fields of resources (`Field::encrypted`, `Field::encrypted_map`):
// the record carries a masked view, never a secret; the form holds what the
// user chose to do, and the payload is the matching operation.
import type { InputSchema } from "@/api/types";

/** What saving does to an encrypted string. */
export type SecretIntent = { kind: "keep" } | { kind: "replace"; value: string } | { kind: "clear" };

export interface SecretValue {
  state: string;
  intent: SecretIntent;
}

/** One row of an encrypted map: a stored key (kept, replaced or removed) or a new one. */
export interface SecretRow {
  key: string;
  stored: boolean;
  action: "keep" | "replace" | "remove";
  value: string;
}

export interface SecretMapValue {
  state: string;
  revision?: string;
  rows: SecretRow[];
}

/** Resource secrets have their own value type; settings secrets do not. */
export const isResourceSecret = (input: InputSchema) => input.type === "secret" && input.valueType === "secret";

const view = (value: unknown): Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

export function secretFormValue(value: unknown): string {
  const state = typeof view(value).state === "string" ? (view(value).state as string) : "unset";
  return JSON.stringify({ state, intent: { kind: "keep" } } satisfies SecretValue);
}

export function secretMapFormValue(value: unknown): string {
  const v = view(value);
  const keys = Array.isArray(v.keys) ? v.keys.filter((k): k is string => typeof k === "string") : [];
  const out: SecretMapValue = {
    state: typeof v.state === "string" ? v.state : "unset",
    revision: typeof v.revision === "string" ? v.revision : undefined,
    rows: keys.map((key) => ({ key, stored: true, action: "keep", value: "" })),
  };
  return JSON.stringify(out);
}

export function parseSecret(text: unknown): SecretValue {
  try {
    const parsed = JSON.parse(String(text)) as SecretValue;
    if (parsed && typeof parsed.state === "string" && parsed.intent) return parsed;
  } catch {
    // Not a secret value: keep.
  }
  return { state: "unset", intent: { kind: "keep" } };
}

export function parseSecretMap(text: unknown): SecretMapValue {
  try {
    const parsed = JSON.parse(String(text)) as SecretMapValue;
    if (parsed && typeof parsed.state === "string" && Array.isArray(parsed.rows)) return parsed;
  } catch {
    // Not a map value: nothing.
  }
  return { state: "unset", rows: [] };
}

/** The write operation of an encrypted string, or `undefined` to leave it out (kept). */
export function secretOperation(text: unknown): unknown {
  const { intent } = parseSecret(text);
  if (intent.kind === "replace") return { replace: intent.value };
  if (intent.kind === "clear") return { clear: true };
  return undefined;
}

/**
 * The write operation of an encrypted map, or `undefined` when nothing
 * changed. Stored keys not kept nor replaced are removed; values are never
 * prefilled, so a kept key is sent by name only.
 */
export function secretMapOperation(text: unknown): unknown {
  const { revision, rows } = parseSecretMap(text);
  const changed = rows.some((r) => !r.stored || r.action !== "keep");
  if (!changed) return undefined;
  const keep: string[] = [];
  const set = Object.create(null) as Record<string, string>;
  for (const row of rows) {
    if (row.stored && row.action === "keep") keep.push(row.key);
    else if (row.action !== "remove" && row.key !== "") set[row.key] = row.value;
  }
  const op: Record<string, unknown> = { set };
  if (keep.length > 0) op.keep = keep;
  if (revision !== undefined) op.revision = revision;
  return op;
}
