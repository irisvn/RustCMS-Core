// Table cells and filters from the table schema.
import { Check, X } from "lucide-react";
import type { ColumnSchema, Envelope, FilterSchema, JsonValue } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Input, NativeSelect } from "@/components/ui/input";
import { display } from "@/lib/wire";
import { t, useT } from "@/i18n";
import { RelationSelect } from "./relation-select";
import { ScriptView } from "@/components/custom";

export const KNOWN_COLUMNS = new Set(["text", "badge", "boolean", "date"]);

/** Classes of a column's header and cells: hidden below its breakpoint (static classes, CSS only). */
const HIDE_BELOW: Record<string, string> = { sm: "hidden sm:table-cell", md: "hidden md:table-cell", lg: "hidden lg:table-cell" };

export function columnClass(column: Pick<ColumnSchema, "hideBelow">): string {
  return (column.hideBelow && HIDE_BELOW[column.hideBelow]) ?? "";
}

export const KNOWN_FILTERS = new Set(["select", "ternary", "date_range"]);

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

export function Cell({ column, record }: { column: ColumnSchema; record: Envelope }) {
  useT();
  const value = record.data[column.name] as JsonValue | undefined;
  // A registered cell draws the value; its column type is the fallback.
  if (column.cell) {
    return (
      <ScriptView kind="cell" name={column.cell} data={value ?? null} record={record.data} props={column.props}
        place={{ kind: "cell", resource: "", field: column.name }}
        fallback={<DefaultCell column={column} record={record} value={value} />} />
    );
  }
  return <DefaultCell column={column} record={record} value={value} />;
}

const BADGE_COLORS = new Set(["primary", "success", "warning", "danger", "gray"]);

function DefaultCell({ column, record, value }: { column: ColumnSchema; record: Envelope; value: JsonValue | undefined }) {
  if (column.relationship) {
    const label = record.labels?.[column.name];
    return <span>{label ?? (value === null || value === undefined ? "" : `#${String(value)}`)}</span>;
  }
  switch (column.type) {
    case "badge": {
      if (value === null || value === undefined) return null;
      const color = column.colors?.find((c) => same(c.value, value))?.color ?? "gray";
      // The colors `BadgeColumn::color` documents; anything else (newer) is gray.
      return <Badge color={BADGE_COLORS.has(color) ? color : "gray"}>{display(column.valueType, value)}</Badge>;
    }
    case "boolean":
      if (value === null || value === undefined) return null;
      return value ? (
        <Check className="size-4 text-foreground" aria-label={t("value.yes")} />
      ) : (
        <X className="size-4 text-muted-foreground" aria-label={t("value.no")} />
      );
    case "date":
      return <span className="whitespace-nowrap">{formatDate(column.valueType, value)}</span>;
    default:
      // "text" and unknown (newer) column types.
      return <span className="line-clamp-2">{display(column.valueType, value)}</span>;
  }
}

function formatDate(kind: string, value: unknown): string {
  if (typeof value !== "string") return "";
  if (kind === "instant") return display(kind, value);
  if (kind === "local_datetime") return value.replace("T", " ").slice(0, 16);
  return value;
}

/** Query-string parameters of a filter, as `filter[field][op]` → value. */
export type FilterParams = Record<string, string>;

const param = (field: string, op: string) => `filter[${field}][${op}]`;

/** Query value of an option (strings raw, numbers as digits). */
const optionParam = (value: JsonValue) => (typeof value === "string" ? value : JSON.stringify(value));

const pad = (n: number) => String(n).padStart(2, "0");
const ymd = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const timed = (kind: string) => kind === "instant" || kind === "local_datetime";

/** The calendar day after `date` (YYYY-MM-DD). */
function nextDay(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  return ymd(new Date(y!, m! - 1, d! + 1));
}

/**
 * Start of `date` in the column's wire kind (instants in the browser's
 * timezone). "Until" a day is sent as `lt` the start of the next day, so the
 * whole day is included at any timestamp precision.
 */
function dayStart(kind: string, date: string): string {
  if (kind === "instant") return new Date(`${date}T00:00:00`).toISOString();
  if (kind === "local_datetime") return `${date}T00:00:00`;
  return date;
}

function dateOf(kind: string, bound: string | undefined, exclusiveEnd = false): string {
  if (!bound) return "";
  let date = bound.slice(0, 10);
  if (kind === "instant") {
    const d = new Date(bound);
    if (Number.isNaN(d.getTime())) return "";
    date = ymd(d);
  }
  if (!exclusiveEnd) return date;
  const [y, m, d] = date.split("-").map(Number);
  return ymd(new Date(y!, m! - 1, d! - 1));
}

export { dayStart, nextDay, dateOf };

export function FilterControl({
  slug,
  filter,
  params,
  onChange,
}: {
  slug: string;
  filter: FilterSchema;
  params: FilterParams;
  onChange: (changes: FilterParams) => void;
}) {
  const t = useT();
  const id = `filter-${filter.name}`;
  switch (filter.type) {
    case "ternary": {
      const key = param(filter.name, "eq");
      return (
        <label className="flex flex-col gap-1 text-xs font-medium">
          {filter.label}
          <NativeSelect id={id} value={params[key] ?? ""} onChange={(e) => onChange({ [key]: e.target.value })}>
            <option value="">{t("filter.any")}</option>
            <option value="true">{t("value.yes")}</option>
            <option value="false">{t("value.no")}</option>
          </NativeSelect>
        </label>
      );
    }
    case "select": {
      const key = param(filter.name, "in");
      const current = params[key] ?? "";
      if (filter.relationship) {
        return (
          <div className="flex flex-col gap-1 text-xs font-medium">
            <label htmlFor={id}>{filter.label}</label>
            <div className="w-full sm:w-48">
              <RelationSelect
                id={id}
                slug={slug}
                field={filter.name}
                value={current === "" ? "" : JSON.stringify(current)}
                onChange={(v) => onChange({ [key]: v === "" ? "" : optionParam(JSON.parse(v) as JsonValue) })}
              />
            </div>
          </div>
        );
      }
      return (
        <label className="flex flex-col gap-1 text-xs font-medium">
          {filter.label}
          <NativeSelect id={id} value={current} onChange={(e) => onChange({ [key]: e.target.value })}>
            <option value="">{t("filter.any")}</option>
            {(filter.options ?? []).map((o) => (
              <option key={optionParam(o.value)} value={optionParam(o.value)}>
                {o.label}
              </option>
            ))}
          </NativeSelect>
        </label>
      );
    }
    case "date_range": {
      const from = param(filter.name, "gte");
      // Timed columns: until = before the next day starts; dates: up to and including.
      const exclusive = timed(filter.valueType);
      const until = param(filter.name, exclusive ? "lt" : "lte");
      return (
        <fieldset className="flex min-w-0 w-full flex-col gap-1 text-xs font-medium sm:w-80">
          <legend>{filter.label}</legend>
          <div className="grid min-w-0 gap-2 sm:grid-cols-2">
            <Input
              type="date"
              aria-label={t("filter.from", { filter: filter.label })}
              value={dateOf(filter.valueType, params[from])}
              onChange={(e) => onChange({ [from]: e.target.value ? dayStart(filter.valueType, e.target.value) : "" })}
            />
            <Input
              type="date"
              aria-label={t("filter.until", { filter: filter.label })}
              value={dateOf(filter.valueType, params[until], exclusive)}
              onChange={(e) => {
                const date = e.target.value;
                const bound = !date ? "" : exclusive ? dayStart(filter.valueType, nextDay(date)) : date;
                onChange({ [until]: bound });
              }}
            />
          </div>
        </fieldset>
      );
    }
    default:
      return null;
  }
}
