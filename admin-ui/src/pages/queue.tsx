// System → Queue (`queue.view`): jobs by status; retry, forget and cancel
// with `queue.manage` (the server checks both on every action).
import { type FormEvent, useState } from "react";
import { type JobAction, type QueueFilters, useJobAction, useQueueJobs, useQueueOverview } from "@/api/queue";
import type { QueueJob } from "@/api/types";
import { Page, PageHeader } from "@/components/layout";
import { ErrorState, Loading } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog } from "@/components/ui/dialog";
import { Input, Label } from "@/components/ui/input";
import { formatDateTime, useT } from "@/i18n";
import { failure, useMounted } from "@/pages/system/shared";

const STATUSES = ["pending", "scheduled", "running", "stalled", "failed"] as const;
type Status = (typeof STATUSES)[number];

const COLORS: Record<Status, string> = {
  pending: "primary",
  scheduled: "info",
  running: "success",
  stalled: "warning",
  failed: "danger",
};

/** Actions a job's status allows. */
function actionsOf(status: string): JobAction[] {
  if (status === "failed") return ["retry", "forget"];
  if (status === "pending" || status === "scheduled" || status === "stalled") return ["cancel"];
  return [];
}

const when = (iso?: string) => (iso ? formatDateTime(new Date(iso)) : "—");

export function QueuePage() {
  const t = useT();
  const overview = useQueueOverview();
  const [draft, setDraft] = useState<QueueFilters>({});
  const [filters, setFilters] = useState<QueueFilters>({});
  const jobs = useQueueJobs(filters);
  const action = useJobAction();
  const mounted = useMounted();
  const [pending, setPending] = useState<{ job: QueueJob; action: JobAction } | null>(null);
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: string } | null>(null);
  const apply = (e: FormEvent) => {
    e.preventDefault();
    setFilters({ status: draft.status || undefined, queue: draft.queue || undefined, kind: draft.kind || undefined });
  };
  const run = async (job: QueueJob, act: JobAction) => {
    setNotice(null);
    try {
      await action.mutateAsync({ id: job.id, action: act });
      if (mounted.current) setNotice({ variant: "default", text: t(`queue.done.${act}`) });
    } catch (e) {
      if (mounted.current) setNotice({ variant: "destructive", text: failure(e).form ?? t("state.errorText") });
    }
  };
  if (overview.isLoading) return <Loading />;
  if (!overview.data) return <ErrorState error={overview.error} />;
  const { queues, workers, canManage } = overview.data;
  const rows = jobs.data?.pages.flatMap((p) => p.data) ?? [];
  return (
    <Page>
      <PageHeader title={t("nav.queue")} description={t("queue.description")} eyebrow={t("nav.system")} />
      {!workers.enabled && <Alert>{t("queue.workersOff")}</Alert>}
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {queues.length === 0 && <p className="text-sm text-muted-foreground">{t("queue.empty")}</p>}
        {queues.map((q) => (
          <Card key={q.name} className="min-w-0">
            <CardHeader>
              <CardTitle>{q.name}</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-wrap gap-2">
              {STATUSES.map((s) => (
                <button key={s} type="button" className="rounded-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  onClick={() => { setDraft({ status: s, queue: q.name }); setFilters({ status: s, queue: q.name }); }}>
                  <Badge color={COLORS[s]}>{t(`queue.status.${s}`)}: {q[s]}</Badge>
                </button>
              ))}
            </CardContent>
          </Card>
        ))}
      </div>
      <Card>
        <CardContent className="p-4">
          <form className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4" onSubmit={apply}>
            <div className="flex min-w-0 flex-col gap-1.5">
              <Label htmlFor="queue-status">{t("queue.filter.status")}</Label>
              <select id="queue-status" className="h-9 rounded-md border border-input bg-background px-2 text-sm"
                value={draft.status ?? ""} onChange={(e) => setDraft({ ...draft, status: e.target.value })}>
                <option value="">{t("queue.any")}</option>
                {STATUSES.map((s) => <option key={s} value={s}>{t(`queue.status.${s}`)}</option>)}
              </select>
            </div>
            {(["queue", "kind"] as const).map((name) => (
              <div key={name} className="flex min-w-0 flex-col gap-1.5">
                <Label htmlFor={`queue-${name}`}>{t(`queue.filter.${name}`)}</Label>
                <Input id={`queue-${name}`} value={draft[name] ?? ""} onChange={(e) => setDraft({ ...draft, [name]: e.target.value })} />
              </div>
            ))}
            <div className="flex items-end gap-2">
              <Button type="submit">{t("audit.apply")}</Button>
              <Button type="button" variant="outline" onClick={() => { setDraft({}); setFilters({}); }}>{t("audit.reset")}</Button>
            </div>
          </form>
        </CardContent>
      </Card>
      {notice && <Alert variant={notice.variant} role="status">{notice.text}</Alert>}
      {jobs.isLoading ? <Loading /> : jobs.error && !jobs.data ? <ErrorState error={jobs.error} /> : (
        <Card className="min-w-0 overflow-x-auto">
          <table className="w-full text-sm" aria-label={t("queue.jobs")}>
            <thead className="border-b border-border text-left text-muted-foreground">
              <tr>
                <th className="p-3 font-medium">{t("queue.col.kind")}</th>
                <th className="p-3 font-medium">{t("queue.col.status")}</th>
                <th className="p-3 font-medium">{t("queue.col.attempts")}</th>
                <th className="p-3 font-medium">{t("queue.col.next")}</th>
                <th className="p-3 font-medium">{t("queue.col.created")}</th>
                <th className="p-3 font-medium">{t("queue.col.error")}</th>
                {canManage && <th className="p-3 font-medium"><span className="sr-only">{t("queue.col.actions")}</span></th>}
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 && (
                <tr><td colSpan={7} className="p-6 text-center text-muted-foreground">{t("queue.noJobs")}</td></tr>
              )}
              {rows.map((job) => (
                <tr key={job.id} className="border-b border-border last:border-0 align-top">
                  <td className="p-3">
                    <div className="font-medium">{job.kind}</div>
                    <div className="text-xs text-muted-foreground">{job.queue} · {job.id}</div>
                    {job.initiatedBy && (
                      <div className="text-xs text-muted-foreground">
                        {t("queue.by", { user: job.initiatedBy.name ?? t("audit.user", { id: job.initiatedBy.id }) })}
                      </div>
                    )}
                  </td>
                  <td className="p-3"><Badge color={COLORS[job.status as Status]}>{t(`queue.status.${job.status as Status}`)}</Badge></td>
                  <td className="p-3 tabular-nums">{job.attempts}/{job.maxAttempts}</td>
                  <td className="p-3">{job.status === "running" ? when(job.reservedUntil) : when(job.availableAt)}</td>
                  <td className="p-3">{when(job.createdAt)}</td>
                  <td className="max-w-xs p-3 break-words text-muted-foreground">{job.lastError ?? ""}</td>
                  {canManage && (
                    <td className="p-3">
                      <div className="flex flex-wrap justify-end gap-2">
                        {actionsOf(job.status).map((act) => (
                          <Button key={act} size="sm" variant={act === "retry" ? "outline" : "destructive"}
                            disabled={action.isPending} onClick={() => setPending({ job, action: act })}>
                            {t(`queue.action.${act}`)}
                          </Button>
                        ))}
                      </div>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
      {jobs.error && jobs.data && <Alert variant="destructive">{jobs.error.message}</Alert>}
      {jobs.hasNextPage && (
        <Button variant="outline" className="self-center" disabled={jobs.isFetchingNextPage} onClick={() => void jobs.fetchNextPage()}>
          {t("audit.more")}
        </Button>
      )}
      <Dialog open={pending !== null} onClose={() => setPending(null)} title={pending ? t(`queue.confirm.${pending.action}`) : ""}>
        {pending && <p className="text-sm text-muted-foreground">{pending.job.kind} · {pending.job.id}</p>}
        {pending && <p className="text-sm text-muted-foreground">{t(`queue.explain.${pending.action}`)}</p>}
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="outline" onClick={() => setPending(null)}>{t("action.cancel")}</Button>
          <Button
            variant={pending?.action === "retry" ? "default" : "destructive"}
            onClick={() => {
              const chosen = pending;
              setPending(null);
              if (chosen) void run(chosen.job, chosen.action);
            }}
          >
            {pending ? t(`queue.action.${pending.action}`) : ""}
          </Button>
        </div>
      </Dialog>
    </Page>
  );
}
