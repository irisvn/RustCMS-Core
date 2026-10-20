// System → Schedules (`schedules.view`): recurring tasks declared by the app,
// their next and last runs and history; Run now with `schedules.manage`.
import { useState } from "react";
import { useUncertain } from "@/api/context";
import { SCHEDULES, useRunNow, useScheduleRuns, useSchedules } from "@/api/schedules";
import type { ScheduleInfo, ScheduleRunInfo } from "@/api/types";
import { Page, PageHeader } from "@/components/layout";
import { AutoRefreshSwitch, formatDuration } from "@/components/refresh";
import { ErrorState, Loading, PausedBanner } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Dialog } from "@/components/ui/dialog";
import { type Text, formatDateTime, t as translate, useT } from "@/i18n";
import { interval, useAutoRefresh, useVisible } from "@/lib/refresh";
import { failure, useMounted } from "@/pages/system/shared";

type RunStatus = "dispatched" | "running" | "succeeded" | "failed" | "skipped" | "cancelled" | "unknown";

const COLORS: Record<RunStatus, string> = {
  dispatched: "info",
  running: "primary",
  succeeded: "success",
  failed: "danger",
  skipped: "warning",
  cancelled: "warning",
  unknown: "warning",
};

const when = (iso?: string) => (iso ? formatDateTime(new Date(iso)) : "—");

function RunBadge({ run }: { run: ScheduleRunInfo }) {
  const t = useT();
  const status = run.status as RunStatus;
  return <Badge color={COLORS[status] ?? "warning"}>{t(`schedules.status.${status}`)}</Badge>;
}

export function SchedulesPage() {
  const t = useT();
  const [auto, setAuto] = useAutoRefresh();
  const visible = useVisible();
  const [confirm, setConfirm] = useState<ScheduleInfo | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const poll = interval(auto, visible, confirm !== null || open !== null);
  const list = useSchedules(poll);
  const runNow = useRunNow();
  const mounted = useMounted();
  const uncertain = useUncertain(SCHEDULES);
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: Text } | null>(null);
  if (list.isLoading) return <Loading />;
  if (!list.data) return <ErrorState error={list.error} />;
  const { data, scheduler, workers, canManage } = list.data;
  const start = async (schedule: ScheduleInfo) => {
    setNotice(null);
    try {
      await runNow.mutateAsync(schedule.name);
      if (mounted.current) setNotice({ variant: "default", text: () => translate("schedules.started", { name: schedule.name }) });
    } catch (e) {
      const f = failure(e);
      if (mounted.current) setNotice({ variant: "destructive", text: () => f.form ?? translate("state.errorText") });
    }
  };
  return (
    <Page>
      <PageHeader title={t("nav.schedules")} description={t("schedules.description")} eyebrow={t("nav.system")}
        actions={<AutoRefreshSwitch on={auto} onChange={setAuto} />} />
      {!scheduler.enabled && <Alert>{t("schedules.off")}</Alert>}
      {!workers && <Alert>{t("schedules.noWorkers")}</Alert>}
      <PausedBanner scope={SCHEDULES} />
      {notice && <Alert variant={notice.variant} role="status">{typeof notice.text === "function" ? notice.text() : notice.text}</Alert>}
      <Card className="min-w-0 overflow-x-auto">
        <table className="w-full text-sm" aria-label={t("nav.schedules")}>
          <thead className="border-b border-border text-left text-muted-foreground">
            <tr>
              <th className="p-3 font-medium">{t("schedules.col.name")}</th>
              <th className="p-3 font-medium">{t("schedules.col.action")}</th>
              <th className="p-3 font-medium">{t("schedules.col.next")}</th>
              <th className="p-3 font-medium">{t("schedules.col.last")}</th>
              {canManage && <th className="p-3 font-medium"><span className="sr-only">{t("queue.col.actions")}</span></th>}
            </tr>
          </thead>
          <tbody>
            {data.length === 0 && (
              <tr><td colSpan={5} className="p-6 text-center text-muted-foreground">{t("schedules.none")}</td></tr>
            )}
            {data.map((s) => (
              <tr key={s.name} className="border-b border-border last:border-0 align-top">
                <td className="p-3">
                  <button type="button" onClick={() => setOpen(s.name)}
                    className="text-left font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                    {s.name}
                  </button>
                  <div className="text-xs text-muted-foreground">{s.rule}</div>
                </td>
                <td className="p-3">
                  {s.action.kind === "job" ? <span className="font-mono text-xs">{s.action.jobKind}</span> : t("schedules.task")}
                  {s.withoutOverlapping && <div className="text-xs text-muted-foreground">{t("schedules.noOverlap")}</div>}
                </td>
                <td className="p-3">{when(s.nextAt)}</td>
                <td className="max-w-sm p-3">
                  {s.last ? (
                    <div className="flex flex-col gap-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <RunBadge run={s.last} />
                        <span>{when(s.last.startedAt)}</span>
                        <span className="tabular-nums text-muted-foreground">{formatDuration(s.last.durationMs)}</span>
                      </div>
                      {s.last.summary && <div className="break-words">{s.last.summary}</div>}
                      {s.last.error && <div className="break-words text-muted-foreground">{s.last.error}</div>}
                    </div>
                  ) : <span className="text-muted-foreground">{t("schedules.never")}</span>}
                </td>
                {canManage && (
                  <td className="p-3 text-right">
                    <Button size="sm" variant="outline" disabled={runNow.isPending || uncertain} onClick={() => setConfirm(s)}>
                      {t("schedules.runNow")}
                    </Button>
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
      <RunsDialog name={open} onClose={() => setOpen(null)} />
      <Dialog open={confirm !== null} onClose={() => setConfirm(null)} title={t("schedules.confirm", { name: confirm?.name ?? "" })}>
        <p className="text-sm text-muted-foreground">{t("schedules.explain")}</p>
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="outline" onClick={() => setConfirm(null)}>{t("action.cancel")}</Button>
          <Button onClick={() => {
            const chosen = confirm;
            setConfirm(null);
            if (chosen) void start(chosen);
          }}>
            {t("schedules.runNow")}
          </Button>
        </div>
      </Dialog>
    </Page>
  );
}

/** The recent runs of one schedule. */
function RunsDialog({ name, onClose }: { name: string | null; onClose: () => void }) {
  const t = useT();
  const runs = useScheduleRuns(name);
  const rows = runs.data?.pages.flatMap((p) => p.data) ?? [];
  return (
    <Dialog open={name !== null} onClose={onClose} title={t("schedules.runs", { name: name ?? "" })} className="max-w-2xl">
      {runs.isLoading ? <Loading /> : runs.error && !runs.data ? <ErrorState error={runs.error} /> : rows.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("schedules.never")}</p>
      ) : (
        <ul className="flex flex-col gap-2 text-sm">
          {rows.map((run) => (
            <li key={run.id} className="flex flex-col gap-1 rounded-md border border-border p-2">
              <div className="flex flex-wrap items-center gap-2">
                <RunBadge run={run} />
                <span>{when(run.startedAt)}</span>
                <span className="text-muted-foreground">{t(`schedules.trigger.${run.trigger === "manual" ? "manual" : "schedule"}`)}</span>
                <span className="tabular-nums text-muted-foreground">{formatDuration(run.durationMs)}</span>
              </div>
              {run.summary && <div className="break-words">{run.summary}</div>}
              {run.error && <div className="break-words text-muted-foreground">{run.error}</div>}
              {run.jobId && <div className="font-mono text-xs text-muted-foreground">{t("schedules.job", { id: run.jobId })}</div>}
            </li>
          ))}
        </ul>
      )}
      {runs.hasNextPage && (
        <Button variant="outline" className="self-center" disabled={runs.isFetchingNextPage} onClick={() => void runs.fetchNextPage()}>
          {t("audit.more")}
        </Button>
      )}
    </Dialog>
  );
}
