// Wire format (docs/core/admin-protocol.md, ADR 0003). Values stay in wire form
// inside forms: i64 and decimal are strings and never pass through `Number`.

import { formatDateTime, t } from "@/i18n";

export type WireKind =
  | "string"
  | "i32"
  | "i64"
  | "f64"
  | "bool"
  | "decimal"
  | "uuid"
  | "date"
  | "local_datetime"
  | "instant"
  | "json";

const I64 = /^-?\d{1,19}$/;
const DECIMAL = /^-?\d+(\.\d+)?$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;
const LOCAL_DATETIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d{1,9})?)?$/;

export const isNumericString = (kind: WireKind) => kind === "i64" || kind === "decimal";

/** Checks a non-empty text entry for `kind`; returns an error message or null. */
export function checkText(kind: WireKind, text: string): string | null {
  switch (kind) {
    case "i32":
      return /^-?\d+$/.test(text) && Number(text) >= -(2 ** 31) && Number(text) <= 2 ** 31 - 1 ? null : t("issue.integer");
    case "i64":
      return I64.test(text) ? null : t("issue.integer");
    case "f64":
      return text.trim() !== "" && Number.isFinite(Number(text)) ? null : t("issue.number");
    case "decimal":
      return DECIMAL.test(text) ? null : t("wire.decimal");
    case "date":
      return DATE.test(text) ? null : t("issue.date");
    case "local_datetime":
      return LOCAL_DATETIME.test(text) ? null : t("wire.datetime");
    default:
      return null;
  }
}

/**
 * Converts what an input holds (text) to the wire value. Empty text is `null`.
 * Numbers that JavaScript cannot hold exactly (i64, decimal) stay strings.
 */
export function fromText(kind: WireKind, text: string): unknown {
  if (text === "") return null;
  switch (kind) {
    case "i32":
    case "f64":
      return Number(text);
    case "instant":
      return localInputToInstant(text);
    case "local_datetime":
      // <input type="datetime-local"> omits seconds.
      return /T\d{2}:\d{2}$/.test(text) ? `${text}:00` : text;
    default:
      return text;
  }
}

/** Converts a wire value to the text an input shows. */
export function toText(kind: WireKind, value: unknown): string {
  if (value === null || value === undefined) return "";
  if (kind === "instant" && typeof value === "string") return instantToLocalInput(value);
  // `datetime-local` (step 1) takes seconds, not fractions.
  if (kind === "local_datetime" && typeof value === "string") return value.slice(0, 19);
  return String(value);
}

const pad = (n: number) => String(n).padStart(2, "0");

/**
 * RFC 3339 instant → `YYYY-MM-DDTHH:MM` **in UTC** for `datetime-local` inputs.
 * Instants are edited in UTC (the input says so): local time is ambiguous in the
 * repeated hour of a DST change and would silently shift the stored instant.
 * Seconds are kept when present.
 */
export function instantToLocalInput(instant: string): string {
  const d = new Date(instant);
  if (Number.isNaN(d.getTime())) return "";
  const seconds = d.getUTCSeconds() ? `:${pad(d.getUTCSeconds())}` : "";
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}T${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}${seconds}`;
}

/** `datetime-local` text, read as UTC → RFC 3339. */
export function localInputToInstant(text: string): string | null {
  const d = new Date(/T\d{2}:\d{2}$/.test(text) ? `${text}:00Z` : `${text}Z`);
  return Number.isNaN(d.getTime()) ? null : d.toISOString().replace(/\.000Z$/, "Z");
}

/** Human display of a wire value in tables and read-only views. */
export function display(kind: string, value: unknown): string {
  if (value === null || value === undefined) return "";
  if (kind === "instant" && typeof value === "string") {
    const d = new Date(value);
    return Number.isNaN(d.getTime()) ? value : formatDateTime(d);
  }
  if (kind === "bool") return value ? t("value.yes") : t("value.no");
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}
