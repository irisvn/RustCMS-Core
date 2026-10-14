// The audit log (auditors: `audit.view_all`) and a record's history.
import { type FormEvent, useState } from "react";
import { type AuditFilters, useAuditLog, useHistory } from "@/api/audit";
import { AuditEntries } from "@/components/audit";
import { Page, PageHeader } from "@/components/layout";
import { ErrorState, Loading } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input, Label } from "@/components/ui/input";
import { useT } from "@/i18n";

const FIELDS = ["event", "subject_type", "subject_key", "actor"] as const;

/** `datetime-local` text as RFC 3339 in UTC (the browser's zone). */
const instant = (local: string) => (local ? new Date(local).toISOString() : undefined);

export function AuditLogPage() {
  const t = useT();
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [filters, setFilters] = useState<AuditFilters>({});
  const log = useAuditLog(filters);
  const apply = (e: FormEvent) => {
    e.preventDefault();
    setFilters({
      event: draft.event || undefined,
      subject_type: draft.subject_type || undefined,
      subject_key: draft.subject_type && draft.subject_key ? draft.subject_key : undefined,
      actor: draft.actor || undefined,
      from: instant(draft.from ?? ""),
      to: instant(draft.to ?? ""),
    });
  };
  const entries = log.data?.pages.flatMap((p) => p.data) ?? [];
  return (
    <Page>
      <PageHeader title={t("nav.audit")} description={t("audit.description")} eyebrow={t("nav.system")} />
      <Card>
        <CardContent className="p-4">
          <form className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3" onSubmit={apply}>
            {FIELDS.map((name) => (
              <div key={name} className="flex min-w-0 flex-col gap-1.5">
                <Label htmlFor={`audit-${name}`}>{t(`audit.filter.${name}`)}</Label>
                <Input id={`audit-${name}`} value={draft[name] ?? ""} inputMode={name === "actor" ? "numeric" : undefined}
                  onChange={(e) => setDraft({ ...draft, [name]: e.target.value })} />
              </div>
            ))}
            {(["from", "to"] as const).map((name) => (
              <div key={name} className="flex min-w-0 flex-col gap-1.5">
                <Label htmlFor={`audit-${name}`}>{t(`audit.filter.${name}`)}</Label>
                <Input id={`audit-${name}`} type="datetime-local" value={draft[name] ?? ""}
                  onChange={(e) => setDraft({ ...draft, [name]: e.target.value })} />
              </div>
            ))}
            <div className="flex items-end gap-2">
              <Button type="submit">{t("audit.apply")}</Button>
              <Button type="button" variant="outline" onClick={() => { setDraft({}); setFilters({}); }}>{t("audit.reset")}</Button>
            </div>
          </form>
        </CardContent>
      </Card>
      {log.isLoading ? <Loading /> : log.error && !log.data ? <ErrorState error={log.error} /> : (
        <Card className="min-w-0 overflow-hidden">
          <AuditEntries entries={entries} />
        </Card>
      )}
      {log.error && log.data && <Alert variant="destructive">{log.error.message}</Alert>}
      {log.hasNextPage && (
        <Button variant="outline" className="self-center" disabled={log.isFetchingNextPage} onClick={() => void log.fetchNextPage()}>
          {t("audit.more")}
        </Button>
      )}
    </Page>
  );
}

/** The history of a record (`abilities.history`): engine events, readable fields only. */
export function RecordHistory({ slug, recordKey }: { slug: string; recordKey: string }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const history = useHistory(slug, recordKey, open);
  const entries = history.data?.pages.flatMap((p) => p.data) ?? [];
  return (
    <Card className="min-w-0">
      <CardHeader className="flex flex-row items-center justify-between gap-3">
        <CardTitle>{t("audit.history")}</CardTitle>
        <Button variant="outline" size="sm" aria-expanded={open} onClick={() => setOpen(!open)}>
          {open ? t("audit.hideHistory") : t("audit.showHistory")}
        </Button>
      </CardHeader>
      {open && (
        <CardContent className="p-0">
          {history.isLoading ? <Loading /> : history.error && !history.data ? <ErrorState error={history.error} /> : (
            <AuditEntries entries={entries} showSubject={false} />
          )}
          {history.error && history.data && <Alert variant="destructive" className="m-3">{history.error.message}</Alert>}
          {history.hasNextPage && (
            <div className="flex justify-center p-3">
              <Button variant="outline" size="sm" disabled={history.isFetchingNextPage} onClick={() => void history.fetchNextPage()}>
                {t("audit.more")}
              </Button>
            </div>
          )}
        </CardContent>
      )}
    </Card>
  );
}
