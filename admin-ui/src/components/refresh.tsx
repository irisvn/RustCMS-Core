// Shared parts of the Queue and Schedules pages: the auto refresh switch, the
// time range of a filter, and how long something took.
import { useId } from "react";
import { Switch } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/input";
import { useT } from "@/i18n";

export function AutoRefreshSwitch({ on, onChange }: { on: boolean; onChange: (on: boolean) => void }) {
  const t = useT();
  const id = useId();
  return (
    <div className="flex items-center gap-2">
      <Switch id={id} checked={on} onCheckedChange={onChange} />
      <Label htmlFor={id}>{t("refresh.auto")}</Label>
    </div>
  );
}

export const RANGES = ["", "1h", "24h", "7d"] as const;
export type Range = (typeof RANGES)[number];

/** The `since` of a range, from now. */
export function sinceOf(range: Range, now: Date = new Date()): string | undefined {
  const hours = range === "1h" ? 1 : range === "24h" ? 24 : range === "7d" ? 168 : 0;
  return hours ? new Date(now.getTime() - hours * 3_600_000).toISOString() : undefined;
}

export function RangeSelect({ id, value, onChange }: { id: string; value: Range; onChange: (range: Range) => void }) {
  const t = useT();
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <Label htmlFor={id}>{t("refresh.range")}</Label>
      <select id={id} className="h-9 rounded-md border border-input bg-background px-2 text-sm"
        value={value} onChange={(e) => onChange(e.target.value as Range)}>
        <option value="">{t("refresh.range.any")}</option>
        <option value="1h">{t("refresh.range.1h")}</option>
        <option value="24h">{t("refresh.range.24h")}</option>
        <option value="7d">{t("refresh.range.7d")}</option>
      </select>
    </div>
  );
}

/** `1.2 s`, `3 min 5 s`. */
export function formatDuration(ms: number | undefined | null): string {
  if (ms === undefined || ms === null) return "—";
  if (ms < 1_000) return `${ms} ms`;
  const s = Math.round(ms / 100) / 10;
  if (s < 60) return `${s} s`;
  const total = Math.round(ms / 1_000);
  const m = Math.floor(total / 60);
  return m < 60 ? `${m} min ${total % 60} s` : `${Math.floor(m / 60)} h ${m % 60} min`;
}
