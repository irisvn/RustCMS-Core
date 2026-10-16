// Dashboard widgets: each loads on its own (one failing never hides the
// others). Counts arrive as integer strings (ADR 0003) and are formatted
// without losing precision; charts are plain SVG shapes styled by classes
// (no inline style: CSP).
import { Link } from "@tanstack/react-router";
import { ArrowDownRight, ArrowUpRight } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useApi } from "@/api/context";
import type { ChartBucket, DashboardView, WidgetData, WidgetInfo } from "@/api/types";
import { useLocale, useT } from "@/i18n";
import { cn } from "@/lib/utils";

const enc = encodeURIComponent;

export function useDashboard() {
  const api = useApi();
  const locale = useLocale();
  return useQuery({ queryKey: ["dashboard", locale], queryFn: () => api.get<DashboardView>("/dashboard") });
}

function useWidget(name: string) {
  const api = useApi();
  const locale = useLocale();
  return useQuery({
    queryKey: ["dashboard", "widget", name, locale],
    queryFn: () => api.get<WidgetData>(`/dashboard/widgets/${enc(name)}`),
  });
}

/** An integer string, grouped for `locale` (big values keep every digit). */
export function formatCount(value: string, locale: string): string {
  if (!/^-?\d+$/.test(value)) return value;
  try {
    return new Intl.NumberFormat(locale).format(BigInt(value));
  } catch {
    return value;
  }
}

const SPAN: Record<number, string> = { 1: "", 2: "sm:col-span-2", 3: "sm:col-span-2 xl:col-span-3", 4: "sm:col-span-2 xl:col-span-4" };

export function DashboardWidgets({ widgets }: { widgets: WidgetInfo[] }) {
  if (widgets.length === 0) return null;
  return (
    <section className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
      {widgets.map((w) => (
        <div key={w.name} className={cn("min-w-0 rounded-xl border border-border bg-card p-5 shadow-xs", SPAN[w.span])}>
          <h2 className="mb-3 text-sm font-medium text-muted-foreground">{w.label}</h2>
          <WidgetBody widget={w} />
        </div>
      ))}
    </section>
  );
}

function WidgetBody({ widget }: { widget: WidgetInfo }) {
  const t = useT();
  const data = useWidget(widget.name);
  if (data.isLoading) return <p className="text-sm text-muted-foreground">{t("dashboard.widgetLoading")}</p>;
  if (!data.data) return <p role="alert" className="text-sm text-destructive">{t("dashboard.widgetFailed")}</p>;
  const d = data.data;
  switch (d.kind) {
    case "stat":
      return <Stat value={d.value} previous={d.previous} />;
    case "chart":
      return <BarChart buckets={d.buckets} label={widget.label} />;
    case "recent":
      return <Recent resource={widget.resource} columns={d.columns} records={d.records} />;
    case "too_many":
      return <p className="text-sm text-muted-foreground">{t("dashboard.tooMany")}</p>;
  }
}

function Stat({ value, previous }: { value: string; previous?: string }) {
  const t = useT();
  const locale = useLocale();
  const up = previous !== undefined && BigInt(value) > BigInt(previous);
  const down = previous !== undefined && BigInt(value) < BigInt(previous);
  return (
    <div className="flex flex-col gap-1">
      <p className="text-3xl font-semibold tabular-nums tracking-tight">{formatCount(value, locale)}</p>
      {previous !== undefined && (
        <p className="flex items-center gap-1 text-xs text-muted-foreground">
          {up && <ArrowUpRight aria-hidden className="size-3.5" />}
          {down && <ArrowDownRight aria-hidden className="size-3.5" />}
          {t("dashboard.previous", { count: formatCount(previous, locale) })}
        </p>
      )}
    </div>
  );
}

const HEIGHT = 120;
const BAR = 10;
const GAP = 4;

export function BarChart({ buckets, label }: { buckets: ChartBucket[]; label: string }) {
  const t = useT();
  const locale = useLocale();
  const counts = buckets.map((b) => Number(b.count));
  const max = Math.max(1, ...counts);
  const width = buckets.length * (BAR + GAP);
  const day = (start: string) => new Date(`${start}T00:00:00Z`).toLocaleDateString(locale, { timeZone: "UTC", month: "short", day: "numeric" });
  return (
    <figure className="flex flex-col gap-2">
      <svg role="img" aria-label={label} viewBox={`0 0 ${width} ${HEIGHT}`} preserveAspectRatio="none" className="h-32 w-full">
        {buckets.map((b, i) => {
          const h = Math.round(((counts[i] ?? 0) / max) * (HEIGHT - 2));
          return (
            <rect key={b.start} x={i * (BAR + GAP)} y={HEIGHT - h} width={BAR} height={Math.max(h, 1)} rx={2} className={h ? "fill-primary" : "fill-muted"}>
              <title>{`${day(b.start)}: ${formatCount(b.count, locale)}`}</title>
            </rect>
          );
        })}
      </svg>
      {buckets.length > 0 && (
        <figcaption className="flex justify-between text-xs text-muted-foreground">
          <span>{day(buckets[0]?.start ?? "")}</span>
          <span>{day(buckets.at(-1)?.start ?? "")}</span>
        </figcaption>
      )}
      <table className="sr-only">
        <caption>{label}</caption>
        <thead><tr><th scope="col">{t("dashboard.period")}</th><th scope="col">{t("dashboard.count")}</th></tr></thead>
        <tbody>{buckets.map((b) => <tr key={b.start}><td>{day(b.start)}</td><td>{formatCount(b.count, locale)}</td></tr>)}</tbody>
      </table>
    </figure>
  );
}

function Recent({ resource, columns, records }: {
  resource: string;
  columns: { name: string; label: string }[];
  records: { key: string; title?: string; values: Record<string, string> }[];
}) {
  const t = useT();
  if (records.length === 0) return <p className="text-sm text-muted-foreground">{t("dashboard.noRecords")}</p>;
  return (
    <ul className="flex flex-col divide-y divide-border">
      {records.map((r) => (
        <li key={r.key} className="flex min-w-0 flex-col gap-0.5 py-2 first:pt-0 last:pb-0">
          <Link to="/r/$slug/records/$key" params={{ slug: resource, key: r.key }} className="truncate text-sm font-medium text-primary hover:underline">
            {r.title ?? `#${r.key}`}
          </Link>
          {columns.length > 0 && (
            <span className="truncate text-xs text-muted-foreground">
              {columns.map((c) => `${c.label}: ${r.values[c.name] ?? "—"}`).join(" · ")}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}
