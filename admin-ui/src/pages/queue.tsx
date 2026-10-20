// System → Queue (`queue.view`): jobs by status and the history of finished
// ones, refreshed every few seconds; retry, forget and cancel with
// `queue.manage` (the server checks both on every action).
import { type FormEvent, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useUncertain } from "@/api/context";
import {
  type JobAction, QUEUE, type QueueFilters, useJobAction, useQueueHistory, useQueueJob, useQueueJobs, useQueueOverview,
} from "@/api/queue";
import type { JobRun, QueueJob } from "@/api/types";
import { Page, PageHeader } from "@/components/layout";
import { AutoRefreshSwitch, type Range, RangeSelect, formatDuration, sinceOf } from "@/components/refresh";
import { ErrorState, Loading, PausedBanner } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog } from "@/components/ui/dialog";
import { Input, Label } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { type Text, formatDateTime, t as translate, useT } from "@/i18n";
import { interval, useAutoRefresh, useVisible } from "@/lib/refresh";
import { failure, useMounted } from "@/pages/system/shared";

const STATUSES = ["pending", "scheduled", "running", "stalled", "failed"] as const;
type Status = (typeof STATUSES)[number];
const FINISHED = ["done", "failed", "cancelled"] as const;
type Finished = (typeof FINISHED)[number];

const COLORS: Record<Status | Finished, string> = {
  pending: "primary",
  scheduled: "info",
  running: "success",
  stalled: "warning",
  failed: "danger",
  done: "success",
  cancelled: "warning",
};

/** Actions a job's status allows. */
function actionsOf(status: string): JobAction[] {
  if (status === "failed") return ["retry", "forget"];
  if (status === "pending" || status === "scheduled" || status === "stalled") return ["cancel"];
  return [];
}

const when = (iso?: string) => (iso ? formatDateTime(new Date(iso)) : "—");
const since = (iso?: string) => (iso ? formatDuration(Date.now() - new Date(iso).getTime()) : "—");

type Draft = { status?: string; queue?: string; kind?: string; range: Range };
const filtersOf = (draft: Draft): QueueFilters => ({
  status: draft.status || undefined,
  queue: draft.queue || undefined,
  kind: draft.kind || undefined,
  since: sinceOf(draft.range),
});

export function QueuePage() {
  const t = useT();
  const client = useQueryClient();
  const [auto, setAuto] = useAutoRefresh();
  const visible = useVisible();
  const [tab, setTab] = useState("jobs");
  const [draft, setDraft] = useState<Draft>({ range: "" });
  const [filters, setFilters] = useState<QueueFilters>({});
  const [historyDraft, setHistoryDraft] = useState<Draft>({ range: "" });
  const [historyFilters, setHistoryFilters] = useState<QueueFilters>({});
  const [pending, setPending] = useState<{ job: QueueJob; action: JobAction } | null>(null);
  const [details, setDetails] = useState<string | null>(null);
  // No refresh while a dialog is open: what it shows stays still.
  const poll = interval(auto, visible, pending !== null || details !== null);
  const overview = useQueueOverview(poll);
  const jobs = useQueueJobs(filters, tab === "jobs" ? poll : false);
  const history = useQueueHistory(historyFilters, tab === "history" ? poll : false);
  const action = useJobAction();
  const mounted = useMounted();
  // Texts are read when shown: a language change applies to notices on screen.
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: Text } | null>(null);
  // A write with an unknown outcome pauses the scope until it is resolved (banner below).
  const uncertain = useUncertain(QUEUE);
  const run = async (job: QueueJob, act: JobAction) => {
    setNotice(null);
    try {
      await action.mutateAsync({ id: job.id, action: act });
      if (mounted.current) setNotice({ variant: "default", text: () => translate(`queue.done.${act}`) });
    } catch (e) {
      const f = failure(e);
      if (mounted.current) setNotice({ variant: "destructive", text: () => f.form ?? translate("state.errorText") });
    }
  };
  if (overview.isLoading) return <Loading />;
  if (!overview.data) return <ErrorState error={overview.error} />;
  const { queues, workers, canManage, lastDay } = overview.data;
  const rows = jobs.data?.pages.flatMap((p) => p.data) ?? [];
  const finished = history.data?.pages.flatMap((p) => p.data) ?? [];
  const browsing = (tab === "jobs" ? jobs.data : history.data)?.pages.length ?? 0;
  const latest = () => void client.resetQueries({ queryKey: ["queue", tab === "jobs" ? "jobs" : "history"] });
  return (
    <Page>
      <PageHeader title={t("nav.queue")} description={t("queue.description")} eyebrow={t("nav.system")}
        actions={<AutoRefreshSwitch on={auto} onChange={setAuto} />} />
      {!workers.enabled && <Alert>{t("queue.workersOff")}</Alert>}
      <p className="text-sm text-muted-foreground">
        {t("queue.lastDay", { done: lastDay.done, failed: lastDay.failed })}
      </p>
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
                  onClick={() => { setTab("jobs"); setDraft({ status: s, queue: q.name, range: "" }); setFilters({ status: s, queue: q.name }); }}>
                  <Badge color={COLORS[s]}>{t(`queue.status.${s}`)}: {q[s]}</Badge>
                </button>
              ))}
            </CardContent>
          </Card>
        ))}
      </div>
      <p className="text-xs text-muted-foreground">{t("queue.delayedHelp")}</p>
      <PausedBanner scope={QUEUE} />
      {notice && <Alert variant={notice.variant} role="status">{typeof notice.text === "function" ? notice.text() : notice.text}</Alert>}
      {browsing > 1 && auto && (
        <Alert role="status">
          {t("refresh.paused")}{" "}
          <button type="button" className="underline" onClick={latest}>{t("refresh.latest")}</button>
        </Alert>
      )}
      <Tabs value={tab} onValueChange={setTab}>
        <TabsList aria-label={t("nav.queue")}>
          <TabsTrigger value="jobs">{t("queue.jobs")}</TabsTrigger>
          <TabsTrigger value="history">{t("queue.history")}</TabsTrigger>
        </TabsList>
        <TabsContent value="jobs" className="flex flex-col gap-4">
          <Filters id="queue" draft={draft} onDraft={setDraft} statuses={STATUSES}
            onApply={(d) => setFilters(filtersOf(d))} onReset={() => { setDraft({ range: "" }); setFilters({}); }} />
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
                        <JobName kind={job.kind} description={job.description} onOpen={() => setDetails(job.id)} />
                        <div className="text-xs text-muted-foreground">{job.queue} · {job.id}</div>
                        {job.initiatedBy && (
                          <div className="text-xs text-muted-foreground">
                            {t("queue.by", { user: job.initiatedBy.name ?? t("audit.user", { id: job.initiatedBy.id }) })}
                          </div>
                        )}
                      </td>
                      <td className="p-3"><Badge color={COLORS[job.status as Status]}>{t(`queue.status.${job.status as Status}`)}</Badge></td>
                      <td className="p-3 tabular-nums">{job.attempts}/{job.maxAttempts}</td>
                      <td className="p-3">
                        {job.status === "running"
                          ? t("queue.startedFor", { at: when(job.startedAt), elapsed: since(job.startedAt) })
                          : job.status === "scheduled" ? t("queue.runsAt", { at: when(job.availableAt) }) : when(job.availableAt)}
                      </td>
                      <td className="p-3">{when(job.createdAt)}</td>
                      <td className="max-w-xs p-3 break-words text-muted-foreground">{job.lastError ?? ""}</td>
                      {canManage && (
                        <td className="p-3">
                          <div className="flex flex-wrap justify-end gap-2">
                            {actionsOf(job.status).map((act) => (
                              <Button key={act} size="sm" variant={act === "retry" ? "outline" : "destructive"}
                                disabled={action.isPending || uncertain} onClick={() => setPending({ job, action: act })}>
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
        </TabsContent>
        <TabsContent value="history" className="flex flex-col gap-4">
          <Filters id="history" draft={historyDraft} onDraft={setHistoryDraft} statuses={FINISHED}
            onApply={(d) => setHistoryFilters(filtersOf(d))} onReset={() => { setHistoryDraft({ range: "" }); setHistoryFilters({}); }} />
          {history.isLoading ? <Loading /> : history.error && !history.data ? <ErrorState error={history.error} /> : (
            <Card className="min-w-0 overflow-x-auto">
              <table className="w-full text-sm" aria-label={t("queue.history")}>
                <thead className="border-b border-border text-left text-muted-foreground">
                  <tr>
                    <th className="p-3 font-medium">{t("queue.col.kind")}</th>
                    <th className="p-3 font-medium">{t("queue.col.status")}</th>
                    <th className="p-3 font-medium">{t("queue.col.attempts")}</th>
                    <th className="p-3 font-medium">{t("queue.col.finished")}</th>
                    <th className="p-3 font-medium">{t("queue.col.duration")}</th>
                    <th className="p-3 font-medium">{t("queue.col.error")}</th>
                  </tr>
                </thead>
                <tbody>
                  {finished.length === 0 && (
                    <tr><td colSpan={6} className="p-6 text-center text-muted-foreground">{t("queue.noHistory")}</td></tr>
                  )}
                  {finished.map((run) => (
                    <tr key={run.id} className="border-b border-border last:border-0 align-top">
                      <td className="p-3">
                        <JobName kind={run.kind} description={run.description} onOpen={() => setDetails(run.jobId)} />
                        <div className="text-xs text-muted-foreground">{run.queue} · {run.jobId}</div>
                      </td>
                      <td className="p-3"><Badge color={COLORS[run.status as Finished]}>{t(`queue.finished.${run.status as Finished}`)}</Badge></td>
                      <td className="p-3 tabular-nums">{run.attempts}</td>
                      <td className="p-3">{when(run.finishedAt)}</td>
                      <td className="p-3 tabular-nums">{formatDuration(run.durationMs)}</td>
                      <td className="max-w-xs p-3 break-words text-muted-foreground">{run.lastError ?? ""}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </Card>
          )}
          {history.hasNextPage && (
            <Button variant="outline" className="self-center" disabled={history.isFetchingNextPage} onClick={() => void history.fetchNextPage()}>
              {t("audit.more")}
            </Button>
          )}
        </TabsContent>
      </Tabs>
      <JobDetails id={details} onClose={() => setDetails(null)} />
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

/** The kind, as a button opening the details, and the job's description. */
function JobName({ kind, description, onOpen }: { kind: string; description?: string; onOpen: () => void }) {
  return (
    <>
      <button type="button" onClick={onOpen}
        className="text-left font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        {kind}
      </button>
      {description && <div className="break-words text-sm">{description}</div>}
    </>
  );
}

function Filters<S extends string>({ id, draft, onDraft, statuses, onApply, onReset }: {
  id: string;
  draft: Draft;
  onDraft: (draft: Draft) => void;
  statuses: readonly S[];
  onApply: (draft: Draft) => void;
  onReset: () => void;
}) {
  const t = useT();
  const label = (s: string) => (id === "history" ? t(`queue.finished.${s as Finished}`) : t(`queue.status.${s as Status}`));
  const apply = (e: FormEvent) => {
    e.preventDefault();
    onApply(draft);
  };
  return (
    <Card>
      <CardContent className="p-4">
        <form className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-5" onSubmit={apply}>
          <div className="flex min-w-0 flex-col gap-1.5">
            <Label htmlFor={`${id}-status`}>{t("queue.filter.status")}</Label>
            <select id={`${id}-status`} className="h-9 rounded-md border border-input bg-background px-2 text-sm"
              value={draft.status ?? ""} onChange={(e) => onDraft({ ...draft, status: e.target.value })}>
              <option value="">{t("queue.any")}</option>
              {statuses.map((s) => <option key={s} value={s}>{label(s)}</option>)}
            </select>
          </div>
          {(["queue", "kind"] as const).map((name) => (
            <div key={name} className="flex min-w-0 flex-col gap-1.5">
              <Label htmlFor={`${id}-${name}`}>{t(`queue.filter.${name}`)}</Label>
              <Input id={`${id}-${name}`} value={draft[name] ?? ""} onChange={(e) => onDraft({ ...draft, [name]: e.target.value })} />
            </div>
          ))}
          <RangeSelect id={`${id}-range`} value={draft.range} onChange={(range) => onDraft({ ...draft, range })} />
          <div className="flex items-end gap-2">
            <Button type="submit">{t("audit.apply")}</Button>
            <Button type="button" variant="outline" onClick={onReset}>{t("audit.reset")}</Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}

/** One job: what it is, when it ran, how it ended, and its earlier runs. */
function JobDetails({ id, onClose }: { id: string | null; onClose: () => void }) {
  const t = useT();
  const detail = useQueueJob(id);
  const job = detail.data?.job;
  const runs: JobRun[] = detail.data?.runs ?? [];
  const first = job ?? runs[0];
  return (
    <Dialog open={id !== null} onClose={onClose} title={t("queue.details")} className="max-w-2xl">
      {detail.isLoading ? <Loading /> : !detail.data ? <ErrorState error={detail.error} /> : (
        <div className="flex flex-col gap-4 text-sm">
          {first && (
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
              <dt className="text-muted-foreground">{t("queue.col.kind")}</dt><dd className="break-all">{first.kind}</dd>
              {first.description && <><dt className="text-muted-foreground">{t("queue.description.label")}</dt><dd className="break-words">{first.description}</dd></>}
              <dt className="text-muted-foreground">{t("queue.filter.queue")}</dt><dd>{first.queue}</dd>
              <dt className="text-muted-foreground">{t("queue.id")}</dt><dd className="break-all font-mono text-xs">{id}</dd>
              <dt className="text-muted-foreground">{t("queue.col.created")}</dt><dd>{when(first.createdAt)}</dd>
              {job && <><dt className="text-muted-foreground">{t("queue.col.status")}</dt><dd>{t(`queue.status.${job.status as Status}`)} · {job.attempts}/{job.maxAttempts}</dd></>}
              {job?.startedAt && <><dt className="text-muted-foreground">{t("queue.started")}</dt><dd>{when(job.startedAt)}</dd></>}
              {job?.lastError && <><dt className="text-muted-foreground">{t("queue.col.error")}</dt><dd className="break-words">{job.lastError}</dd></>}
            </dl>
          )}
          <h3 className="font-semibold">{t("queue.history")}</h3>
          {runs.length === 0 ? <p className="text-muted-foreground">{t("queue.noHistory")}</p> : (
            <ul className="flex flex-col gap-2">
              {runs.map((run) => (
                <li key={run.id} className="rounded-md border border-border p-2">
                  <Badge color={COLORS[run.status as Finished]}>{t(`queue.finished.${run.status as Finished}`)}</Badge>{" "}
                  {when(run.finishedAt)} · {formatDuration(run.durationMs)} · {t("queue.col.attempts")}: {run.attempts}
                  {run.lastError && <div className="mt-1 break-words text-muted-foreground">{run.lastError}</div>}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </Dialog>
  );
}
