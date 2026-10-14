// Audit entries: who did what, when, and what changed (values as stored,
// `[redacted]` where the server withheld them).
import { ChevronDown, ChevronRight } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { usePanel } from "@/api/queries";
import type { AuditEntryView } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { formatDateTime, has, useT } from "@/i18n";

/** The event in words (unknown events, e.g. an application's own, as written). */
export function useEventLabel() {
  const t = useT();
  return (event: string) => {
    if (event.startsWith("action:")) return t("audit.event.action", { name: event.slice("action:".length) });
    const key = `audit.event.${event}`;
    return has(key) ? t(key) : event;
  };
}

const value = (v: unknown) => (v === null || v === undefined ? "∅" : typeof v === "string" ? v : JSON.stringify(v));

function Changes({ changes }: { changes: Record<string, unknown> }) {
  const t = useT();
  if (changes.overflow === true) {
    const fields = Array.isArray(changes.fields) ? changes.fields.map(String) : [];
    return <p className="text-sm text-muted-foreground">{t("audit.tooLarge", { fields: fields.join(", ") })}</p>;
  }
  const rows = Object.entries(changes);
  if (rows.length === 0) return null;
  return (
    <table className="w-full table-fixed text-left text-xs">
      <thead className="text-muted-foreground">
        <tr>
          <th scope="col" className="w-1/4 py-1 pr-3 font-medium">{t("audit.field")}</th>
          <th scope="col" className="py-1 pr-3 font-medium">{t("audit.before")}</th>
          <th scope="col" className="py-1 font-medium">{t("audit.after")}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map(([field, pair]) => {
          const [before, after] = Array.isArray(pair) ? pair : [undefined, pair];
          return (
            <tr key={field} className="border-t border-border align-top">
              <th scope="row" className="break-all py-1 pr-3 font-medium">{field}</th>
              <td className="break-all py-1 pr-3 font-mono">{value(before)}</td>
              <td className="break-all py-1 font-mono">{value(after)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function Entry({ entry, showSubject }: { entry: AuditEntryView; showSubject: boolean }) {
  const t = useT();
  const label = useEventLabel();
  const [open, setOpen] = useState(false);
  const actor = entry.actor.kind === "user"
    ? entry.actor.name ?? t("audit.user", { id: entry.actor.id ?? "?" })
    : entry.actor.kind === "system" ? t("audit.system") : t("audit.guest");
  const details = entry.changes || entry.meta || entry.ip || entry.userAgent || entry.requestId;
  return (
    <li className="border-b border-border px-4 py-3 last:border-0">
      <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-sm">
        {details ? (
          <Button variant="ghost" size="icon" className="size-6" aria-expanded={open} aria-label={open ? t("audit.hide") : t("audit.show")} onClick={() => setOpen(!open)}>
            {open ? <ChevronDown /> : <ChevronRight />}
          </Button>
        ) : <span className="size-6" aria-hidden="true" />}
        <time dateTime={entry.at} className="tabular-nums text-muted-foreground">{formatDateTime(new Date(entry.at))}</time>
        <span className="font-medium">{actor}</span>
        {entry.actor.credential && <Badge className="border border-border bg-background text-muted-foreground">{entry.actor.credential}</Badge>}
        <span>{label(entry.event)}</span>
        {showSubject && <Subject entry={entry} />}
      </div>
      {open && (
        <div className="ml-9 mt-2 flex min-w-0 flex-col gap-2">
          {entry.changes && <Changes changes={entry.changes} />}
          {entry.meta && <pre className="overflow-x-auto rounded-md bg-muted p-2 text-xs">{JSON.stringify(entry.meta, null, 2)}</pre>}
          <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 text-xs text-muted-foreground">
            {entry.ip && <><dt>{t("audit.ip")}</dt><dd className="break-all">{entry.ip}</dd></>}
            {entry.userAgent && <><dt>{t("audit.userAgent")}</dt><dd className="break-all">{entry.userAgent}</dd></>}
            {entry.requestId && <><dt>{t("audit.requestId")}</dt><dd className="break-all font-mono">{entry.requestId}</dd></>}
          </dl>
        </div>
      )}
    </li>
  );
}

/** The subject; a link to the record when it is a resource the user may list (its page checks the rest). */
function Subject({ entry }: { entry: AuditEntryView }) {
  const panel = usePanel();
  const text = `${entry.subjectType}${entry.subjectKey ? ` #${entry.subjectKey}` : ""}`;
  const listed = panel.data?.navigation.some((r) => r.slug === entry.subjectType);
  if (listed && entry.subjectKey && entry.event !== "deleted" && entry.event !== "force_deleted") {
    return (
      <Link to="/r/$slug/records/$key" params={{ slug: entry.subjectType, key: entry.subjectKey }} className="min-w-0 break-all text-primary hover:underline">
        {text}
      </Link>
    );
  }
  return <span className="min-w-0 break-all text-muted-foreground">{text}</span>;
}

export function AuditEntries({ entries, showSubject = true }: { entries: AuditEntryView[]; showSubject?: boolean }) {
  const t = useT();
  if (entries.length === 0) return <p className="p-6 text-center text-sm text-muted-foreground">{t("audit.empty")}</p>;
  return <ul aria-label={t("audit.entries")}>{entries.map((e) => <Entry key={e.id} entry={e} showSubject={showSubject} />)}</ul>;
}
