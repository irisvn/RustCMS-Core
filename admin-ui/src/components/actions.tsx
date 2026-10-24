// Actions of one record outside the list page (record pages, relation
// tables): the first primary action the record offers with its label, the
// others under ⋯ (destructive ones last), and what running one takes
// (confirmation, form dialog, the outcome's message and page).
import { Ellipsis } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { useRunAction } from "@/api/queries";
import type { ActionOutcome, ActionSchema, Envelope } from "@/api/types";
import { SchemaFormDialog } from "@/components/schema-form-dialog";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuItem, DropdownMenuSeparator } from "@/components/ui/menu";
import { errorText, formatCount, later, show, t as translate, type Text, useT } from "@/i18n";
import { openInNewTab, openableUrl } from "@/lib/open-url";

/** An action's label as a dialog title or button: without a trailing ellipsis (`Schedule…`). */
export const actionTitle = (label: string) => label.replace(/\s*(…|\.\.\.)\s*$/u, "") || label;

/** The label of the button that runs `action` from its form. */
export const submitLabelOf = (action: ActionSchema) => action.submitLabel ?? actionTitle(action.label);

const recordCount = (n: number) => translate("list.records", { count: n, n: formatCount(n) });

/** The summary of an action's outcome. */
export function outcomeMessage(label: string, outcome: ActionOutcome): string {
  const parts = [translate("action.outcomeDone", { action: label, records: recordCount(outcome.processed.length) })];
  if (outcome.denied.length) parts.push(translate("action.outcomeDenied", { records: recordCount(outcome.denied.length) }));
  if (outcome.missing.length) parts.push(translate("action.outcomeMissing", { records: recordCount(outcome.missing.length) }));
  return translate("action.outcome", { parts: parts.join(translate("list.separator")) });
}

export interface ActionNotice {
  variant: "default" | "destructive";
  text: Text;
  /** The page the outcome opened (a fallback link: the browser may block the tab). */
  link?: string;
}

/** Whether `value` is an action's answer (a run recovered by "Retry safely"). */
export function isOutcome(value: unknown): value is ActionOutcome {
  return typeof value === "object" && value !== null && Array.isArray((value as ActionOutcome).processed);
}

/** The action a re-sent write ran (`…/actions/{name}`), or null. */
export function recoveredAction(path: string): string | null {
  const m = /\/actions\/([^/?]+)$/.exec(path);
  return m ? decodeURIComponent(m[1]!) : null;
}

/** The notice of an outcome (the handler's message first); its page opens in a new tab. */
export function outcomeNotice(label: string, outcome: ActionOutcome): ActionNotice {
  // Answered after the commit: the page opens now.
  const link = openableUrl(outcome.open) ? outcome.open : undefined;
  if (link) openInNewTab(link);
  const text = () => [outcome.message, outcomeMessage(label, outcome)].filter(Boolean).join(" ");
  return { variant: outcome.processed.length ? "default" : "destructive", text, link };
}

export function ActionNoticeView({ notice }: { notice: ActionNotice }) {
  const t = useT();
  return (
    <Alert variant={notice.variant}>
      {show(notice.text)}
      {notice.link && (
        <>
          {" "}
          <a href={notice.link} target="_blank" rel="noopener noreferrer" className="font-medium underline">{t("action.openLink")}</a>
        </>
      )}
    </Alert>
  );
}

/** The actions `record` offers: the first primary one labelled, the others under ⋯, destructive ones last. */
export function RecordActions({ actions, record, disabled, onStart, size = "sm" }: {
  actions: ActionSchema[];
  record: Envelope;
  disabled?: boolean;
  onStart: (action: ActionSchema) => void;
  size?: "sm" | "default";
}) {
  const t = useT();
  const offered = actions.filter((a) => record.abilities[a.name] === true);
  const primary = offered.find((a) => a.primary);
  const rest = offered.filter((a) => a !== primary);
  const safe = rest.filter((a) => a.color !== "danger");
  const destructive = rest.filter((a) => a.color === "danger");
  return (
    <>
      {primary && (
        <Button size={size} variant={size === "sm" ? "ghost" : "outline"} disabled={disabled} onClick={() => onStart(primary)}>
          {primary.label}
        </Button>
      )}
      {rest.length > 0 && (
        <DropdownMenu trigger={<Ellipsis />} variant={size === "sm" ? "ghost" : "outline"} size="icon" align="end" aria-label={t("list.moreActions", { record: record.title ?? record.key })}>
          {safe.map((a) => (
            <DropdownMenuItem key={a.name} disabled={disabled} onSelect={() => onStart(a)}>{a.label}</DropdownMenuItem>
          ))}
          {safe.length > 0 && destructive.length > 0 && <DropdownMenuSeparator />}
          {destructive.map((a) => (
            <DropdownMenuItem key={a.name} destructive disabled={disabled} onSelect={() => onStart(a)}>{a.label}</DropdownMenuItem>
          ))}
        </DropdownMenu>
      )}
    </>
  );
}

/**
 * Runs actions of `slug` on single records: asks for confirmation or the
 * form first, then shows the outcome. `dialogs` must be rendered.
 */
export function useActionRunner(slug: string, offered?: (action: ActionSchema, key: string) => boolean) {
  // Whether the records are still shown and offer the action (the rows may
  // have been refetched while a dialog was open): read when the action is sent.
  const stillOffered = useRef(offered);
  stillOffered.current = offered;
  const current = (action: ActionSchema, keys: string[]) => !stillOffered.current || keys.every((k) => stillOffered.current!(action, k));
  const t = useT();
  const run = useRunAction(slug);
  // Completions act only while the page that started them is shown.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const [confirm, setConfirm] = useState<{ action: ActionSchema; keys: string[] } | null>(null);
  const [form, setForm] = useState<{ action: ActionSchema; keys: string[] } | null>(null);
  const [notice, setNotice] = useState<ActionNotice | null>(null);

  const execute = async (action: ActionSchema, keys: string[]) => {
    setNotice(null);
    if (!current(action, keys)) {
      setNotice({ variant: "destructive", text: later("list.changedMeanwhile") });
      return;
    }
    try {
      const outcome = await run.mutateAsync({ action: action.name, keys });
      if (mounted.current) setNotice(outcomeNotice(action.label, outcome));
    } catch (e) {
      if (mounted.current) setNotice({ variant: "destructive", text: errorText(e, "action.failed") });
    }
  };
  const start = (action: ActionSchema, keys: string[]) => {
    if (action.form) setForm({ action, keys });
    else if (action.confirm) setConfirm({ action, keys });
    else void execute(action, keys);
  };
  const dialogs: ReactNode = (
    <>
      <SchemaFormDialog
        open={form !== null}
        onClose={() => setForm(null)}
        title={form ? actionTitle(form.action.label) : ""}
        form={form?.action.form}
        slug={slug}
        optionsPath={(field) => `/resources/${encodeURIComponent(slug)}/actions/${encodeURIComponent(form?.action.name ?? "")}/fields/${encodeURIComponent(field)}/options`}
        submitLabel={form ? submitLabelOf(form.action) : ""}
        onSubmit={async (data) => {
          if (!form) return;
          if (!current(form.action, form.keys)) throw new Error(translate("list.changedMeanwhile"));
          setNotice(null);
          const outcome = await run.mutateAsync({ action: form.action.name, keys: form.keys, data });
          if (mounted.current) setNotice(outcomeNotice(form.action.label, outcome));
        }}
      >
        {form?.action.confirm && <p className="text-sm text-muted-foreground">{form.action.confirm}</p>}
      </SchemaFormDialog>
      <Dialog open={confirm !== null} onClose={() => setConfirm(null)} title={confirm ? actionTitle(confirm.action.label) : ""}>
        <p className="text-sm text-muted-foreground">{confirm?.action.confirm}</p>
        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => setConfirm(null)}>{t("action.cancel")}</Button>
          <Button onClick={() => {
            const current = confirm;
            setConfirm(null);
            if (current) void execute(current.action, current.keys);
          }}>
            {t("action.confirm")}
          </Button>
        </div>
      </Dialog>
    </>
  );
  /** For `PausedBanner`: shows the outcome of a run recovered by "Retry safely". */
  const recovered = (actions: ActionSchema[]) => (result: unknown, pending: { path: string }) => {
    const name = recoveredAction(pending.path);
    if (!name || !isOutcome(result) || !mounted.current) return false;
    setNotice(outcomeNotice(actions.find((a) => a.name === name)?.label ?? name, result));
    // The banner still says the change was applied.
    return false;
  };
  return { start, pending: run.isPending, notice, dialogs, recovered };
}
