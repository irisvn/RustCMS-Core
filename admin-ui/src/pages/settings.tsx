// A settings page: the group's form, saved with the version it was loaded at.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { ApiError } from "@/api/client";
import { settingsScope, useSaveSettings, useSettings } from "@/api/settings";
import type { SettingsResponse } from "@/api/types";
import { Page, PageHeader } from "@/components/layout";
import { MailTest } from "@/components/mail-test";
import { ErrorState, Loading, PausedBanner, VersionBanner, compatibility } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { Components, gridCols } from "@/renderers/form";
import { type SecretIntent, secretPayload } from "@/renderers/secret";
import { type FormValues, defaultValues, inputsOf, payloadOf, relabel } from "@/schema/form";
import { editResolver } from "@/schema/resolver";
import { useLocale, useT } from "@/i18n";

export function SettingsPage({ group }: { group: string }) {
  const settings = useSettings(group);
  // The last answer stays shown when a refetch (another language) fails: the draft is kept.
  const [last, setLast] = useState<SettingsResponse | undefined>(undefined);
  useEffect(() => {
    if (settings.data) setLast(settings.data);
  }, [settings.data]);
  const latest = settings.data ?? last;
  if (!latest) return settings.isLoading ? <Loading /> : <ErrorState error={settings.error} />;
  const refetch = async () => (await settings.refetch()).data;
  // Keyed by group only: refetches (another language, window focus) never reset a draft.
  return <SettingsForm key={group} group={group} latest={latest} refreshError={settings.isError ? settings.error : null} refetch={refetch} />;
}

function SettingsForm({ group, latest, refreshError, refetch }: {
  group: string;
  latest: SettingsResponse;
  refreshError: unknown;
  refetch: () => Promise<SettingsResponse | undefined>;
}) {
  const t = useT();
  // What the form was loaded from: its structure, values and version.
  const [base, setBase] = useState(latest);
  const components = base.form.components;
  const inputs = useMemo(() => inputsOf(components), [components]);
  // Permissions and compatibility as the server states them now.
  const editable = latest.abilities.update && !compatibility(latest).readOnly && !compatibility(base).readOnly;
  // Only changed fields are sent, so only they are checked.
  const dirty = useRef<(name: string) => boolean>(() => true);
  const resolver = useMemo(() => editResolver(inputs, true, (name) => dirty.current(name)), [inputs]);
  const form = useForm<FormValues>({ defaultValues: defaultValues(inputs, base.data), resolver });
  dirty.current = (name) => form.getFieldState(name).isDirty;
  const [intents, setIntents] = useState<Record<string, SecretIntent>>({});
  // Inputs with background work (media uploads): saving waits for them.
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const onBusy = useCallback((name: string, working: boolean) => {
    setBusy((current) => {
      if (current.has(name) === working) return current;
      const next = new Set(current);
      if (working) next.add(name);
      else next.delete(name);
      return next;
    });
  }, []);
  const [stale, setStale] = useState(false);
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: () => string } | null>(null);
  const save = useSaveSettings(group);
  // Set on every mount (StrictMode mounts twice): completions only apply while shown.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  // Messages of the last refused save, set again in a new language.
  const refused = useRef<ApiError | null>(null);
  const locale = useLocale();
  useEffect(() => {
    for (const [field, messages] of Object.entries(refused.current?.fieldErrors ?? {})) {
      if (form.getFieldState(field).error?.type === "server") form.setError(field, { type: "server", message: messages.join(" ") });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [locale]);

  /** Starts over from what is stored now (after someone else's change); the draft stays if that fails. */
  const reload = async () => {
    const current = await refetch().catch(() => undefined);
    if (!mounted.current) return;
    if (!current) {
      setNotice({ variant: "destructive", text: () => t("state.errorText") });
      return;
    }
    setBase(current);
    form.reset(defaultValues(inputsOf(current.form.components), current.data));
    setIntents({});
    setStale(false);
    setNotice(null);
  };

  const submit = form.handleSubmit(async (values) => {
    if (busy.size > 0) return;
    setNotice(null);
    refused.current = null;
    const empty = Object.entries(intents).find(([, i]) => i.kind === "replace" && i.value === "");
    if (empty) {
      form.setError(empty[0], { type: "client", message: t("secret.emptyReplace") });
      return;
    }
    const dirty = Object.fromEntries(inputs.map((i) => [i.name, form.getFieldState(i.name).isDirty]));
    const data = { ...payloadOf(inputs, values, dirty), ...secretPayload(intents) };
    try {
      const saved = await save.mutateAsync({ version: base.version, data });
      if (!mounted.current) return;
      setBase(saved);
      form.reset(defaultValues(inputsOf(saved.form.components), saved.data));
      setIntents({});
      setNotice({ variant: "default", text: () => t("settings.saved") });
    } catch (e) {
      if (!mounted.current) return;
      if (e instanceof ApiError && e.status === 422) {
        if (e.hasIssue("stale")) {
          // The draft (secrets included) is kept until the user reloads.
          setStale(true);
          return;
        }
        refused.current = e;
        const known = new Set(inputs.map((i) => i.name));
        for (const [field, messages] of Object.entries(e.fieldErrors)) {
          if (known.has(field)) form.setError(field, { type: "server", message: messages.join(" ") });
        }
        const other = () => [...e.formErrors, ...Object.entries(e.fieldErrors).filter(([f]) => !known.has(f)).map(([f, m]) => `${f}: ${m.join(" ")}`)];
        if (other().length) setNotice({ variant: "destructive", text: () => other().join(" ") });
      } else {
        setNotice({ variant: "destructive", text: () => (e instanceof Error ? e.message : t("record.saveFailed")) });
      }
    }
  });

  // The language changed: same form and values, labels of the latest answer.
  const shown = relabel(components, latest.form.components);
  return (
    <Page className="max-w-4xl">
      <form className="flex min-w-0 flex-col gap-6" onSubmit={(e) => void submit(e)} noValidate>
        <PageHeader title={latest.label} eyebrow={t("nav.settings")} description={editable ? t("settings.description") : t("settings.readOnly")} />
        <VersionBanner schema={base} />
        <PausedBanner scope={settingsScope(group)} />
        {stale && (
          <Alert variant="warning">
            {t("settings.stale")}{" "}
            <button type="button" className="underline" onClick={() => void reload()}>{t("system.discard")}</button>
          </Alert>
        )}
        {refreshError instanceof Error && <Alert variant="warning">{t("record.refreshFailed", { error: refreshError.message })}</Alert>}
        {notice && <Alert variant={notice.variant}>{notice.text()}</Alert>}
        <Card>
          <CardContent className={cn("grid grid-cols-1 gap-x-6 gap-y-6 p-5 sm:p-6", gridCols(base.form.columns))}>
            <Components
              components={shown}
              ctx={{
                slug: `settings:${group}`,
                control: form.control,
                errors: form.formState.errors,
                readOnly: !editable || save.isPending,
                submitCount: form.formState.submitCount,
                onBusy,
                secrets: {
                  status: base.secrets,
                  intents,
                  setIntent: (name, intent) => {
                    form.clearErrors(name);
                    setIntents((current) => ({ ...current, [name]: intent }));
                  },
                },
              }}
            />
          </CardContent>
        </Card>
        {editable && (
          <div className="flex flex-wrap items-center gap-3 border-t border-border pt-5">
            <Button type="submit" disabled={save.isPending || stale || busy.size > 0}>
              {save.isPending ? t("record.saving") : t("settings.save")}
            </Button>
          </div>
        )}
      </form>
      {latest.abilities.test && <MailTest />}
    </Page>
  );
}
