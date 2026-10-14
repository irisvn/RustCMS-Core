import { Link, useNavigate } from "@tanstack/react-router";
import { Pencil, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { ApiError } from "@/api/client";
import { useUncertain } from "@/api/context";
import { type Via, useDeleteRecord, useRecord, useSaveRecord, useSchema } from "@/api/queries";
import { RelationSection, withoutInput } from "./relations";
import { RecordHistory } from "./audit";
import type { Envelope, InputSchema, ResourceSchema } from "@/api/types";
import { ErrorState, Loading, PausedBanner, VersionBanner, compatibility } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Button, buttonVariants } from "@/components/ui/button";
import { Page, PageHeader } from "@/components/layout";
import { Card, CardContent } from "@/components/ui/card";
import { Dialog } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { Components, ReadOnlyComponents, gridCols } from "@/renderers/form";
import { type FormValues, defaultValues, inputsOf, payloadOf, protectLossyJson, relabel } from "@/schema/form";
import { errorText, show, type Text, useLocale, useT } from "@/i18n";
import { editResolver } from "@/schema/resolver";
import type { PendingWrite } from "@/app/session";


function useMounted() {
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  return mounted;
}

const linkClass = buttonVariants({ variant: "outline" });

/** Create (`recordKey` undefined) or edit form. */
export function FormPage({ slug, recordKey, via }: { slug: string; recordKey?: string; via?: Via }) {
  const editing = recordKey !== undefined;
  const schema = useSchema(slug, editing ? "edit" : "create", recordKey);
  // Through a relation: the parent's view schema says which field the server sets.
  const parentSchema = useSchema(via?.parent ?? slug, "view", via?.record, { enabled: via !== undefined });
  const binding = via ? parentSchema.data?.relations?.find((r) => r.name === via.relation && r.resource === slug) : undefined;
  // An editor may update without viewing: then the form starts empty (only changed fields are sent).
  // A blind form, once shown, stays blind (and mounted) if a refetch grants `view`:
  // loading the record would replace the page and lose the draft.
  const identity = `${slug}:${recordKey ?? ""}`;
  const shownBlind = useRef<string | null>(null);
  const viewable = !editing || (schema.data?.abilities.view === true && shownBlind.current !== identity);
  const record = useRecord(slug, recordKey ?? "", { enabled: editing && viewable && !!schema.data });

  if (schema.isLoading || (editing && viewable && record.isLoading) || (via && parentSchema.isLoading)) return <Loading />;
  if (via && (!binding || !binding.create)) {
    return <ErrorState error={parentSchema.error ?? new ApiError(403, { code: "forbidden", message: "" })} />;
  }
  // Errors replace the page only when there is nothing to show: a failed background
  // refresh keeps the form (and the user's draft) mounted.
  if (!schema.data) return <ErrorState error={schema.error} />;
  if (editing && viewable && !record.data) return <ErrorState error={record.error} />;
  if ((editing && !schema.data.abilities.update) || (!editing && !schema.data.abilities.create)) {
    return <ErrorState error={new ApiError(403, { code: "forbidden", message: "" })} />;
  }
  const refreshError = schema.error ?? (editing && viewable ? record.error : null);
  if (editing && !viewable) shownBlind.current = identity;
  return (
    <RecordForm
      refreshError={refreshError}
      // Keyed by identity only: refetches (window focus, failed saves) never reset a draft.
      key={identity}
      slug={slug}
      recordKey={recordKey}
      via={via && binding ? { ...via, field: binding.field } : undefined}
      schema={schema.data}
      record={editing && viewable ? record.data : undefined}
    />
  );
}

function RecordForm({
  slug,
  recordKey,
  schema,
  record,
  refreshError,
  via,
}: {
  slug: string;
  recordKey?: string;
  schema: ResourceSchema;
  record?: Envelope;
  refreshError?: unknown;
  /** Creating through a relation; its field is set by the server, so not shown. */
  via?: Via & { field: string };
}) {
  const t = useT();
  const navigate = useNavigate();
  // Completion redirects only apply while the page that started the write is shown.
  const mounted = useMounted();
  const editing = recordKey !== undefined;
  // Fixed at mount, like the defaults: JSON values that lost precision on the way here stay read-only.
  const [components] = useState(() =>
    protectLossyJson(via ? withoutInput(schema.form.components, via.field) : schema.form.components, record?.data),
  );
  const inputs = useMemo(() => inputsOf(components), [components]);
  const { readOnly } = compatibility(schema);
  // Blind edit (update without view): defaults are placeholders, so what the user
  // touched is submitted, even when it equals a placeholder (false, empty).
  const blind = editing && !record;
  const touched = useRef(new Set<string>());
  const dirty = useRef<(name: string) => boolean>(() => true);
  const resolver = useMemo(() => editResolver(inputs, editing, (name) => dirty.current(name)), [inputs, editing]);
  const form = useForm<FormValues>({ defaultValues: defaultValues(inputs, record?.data), resolver });
  // Placeholders are not values: on create (database defaults apply to untouched
  // fields), on a blind edit, and for write-only inputs, only what the user
  // touched is submitted, even when it equals a placeholder (false, empty).
  // On create, what the form shows as a definite value is sent: declared defaults
  // (equal to the database's), and non-nullable toggles/checkboxes, whose "off"
  // is a real value (the entity declares any other default). Other untouched
  // inputs are left out so database defaults and hooks apply. Write-only inputs
  // are only ever sent when touched.
  const shownAsSet = (i: InputSchema) =>
    !i.writeOnly && (i.default !== undefined || ((i.type === "toggle" || i.type === "checkbox") && !i.nullable));
  const byIntent = (name: string) =>
    inputs.some((i) => i.name === name && (i.writeOnly || (!editing && !shownAsSet(i)))) || blind;
  const touchedOrShown = (name: string) =>
    touched.current.has(name) || (!editing && inputs.some((i) => i.name === name && shownAsSet(i)));
  dirty.current = (name) =>
    byIntent(name) ? touchedOrShown(name) : !editing || form.getFieldState(name).isDirty;
  // This form's identity: a retried create continues here only if it came from here.
  const [origin] = useState(() => crypto.randomUUID());
  const save = useSaveRecord(slug, recordKey, origin, via);
  // Read when shown: messages follow a change of language.
  const [formErrors, setFormErrors] = useState<() => string[]>(() => () => []);
  // The last refused save: its field messages are set again in a new language.
  const refused = useRef<ApiError | null>(null);
  const locale = useLocale();
  useEffect(() => {
    const known = new Set(inputsOf(components).map((i) => i.name));
    for (const [field, messages] of Object.entries(refused.current?.fieldErrors ?? {})) {
      if (known.has(field) && form.getFieldState(field).error?.type === "server") {
        form.setError(field, { type: "server", message: messages.join(" ") });
      }
    }
    // The panel's own checks run again, in the new language.
    const shown = Object.keys(form.formState.errors).filter((f) => form.getFieldState(f).error?.type !== "server");
    if (shown.length) void form.trigger(shown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [locale]);
  // Inputs with background work (uploads): saving waits for them.
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
  // After a write with an unknown outcome, resubmitting could create a duplicate.
  const uncertain = useUncertain(slug);

  // After a failed save, the first input with an error (in form order) gets the
  // focus, once enclosing tabs have opened it.
  const focusFirstError = () =>
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const first = inputs.find((i) => form.getFieldState(i.name).error);
        if (first) document.getElementById(`field-${first.name}`)?.focus();
      }),
    );

  // After a save (or a retried one that went through).
  const afterSave = async (saved: Envelope) => {
    if (via) {
      // Back to the parent, where the new record is listed.
      await navigate({ to: "/r/$slug/records/$key", params: { slug: via.parent, key: via.record } });
    } else if (saved.abilities.view) {
      await navigate({ to: "/r/$slug/records/$key", params: { slug, key: saved.key } });
    } else {
      await navigate({ to: "/r/$slug", params: { slug } });
    }
  };

  const submit = form.handleSubmit(async (values) => {
    if (busy.size > 0) return;
    setFormErrors(() => () => []);
    refused.current = null;
    const changed = Object.fromEntries(inputs.map((i) => [i.name, dirty.current(i.name)]));
    const payload = payloadOf(inputs, values, changed);
    // An untouched declared default goes out exactly as the server stated it
    // (no round trip through the input's text form).
    if (!editing) {
      for (const i of inputs) {
        if (i.default !== undefined && !touched.current.has(i.name) && i.name in payload) payload[i.name] = i.default;
      }
    }
    try {
      const saved = await save.mutateAsync(payload);
      if (mounted.current) await afterSave(saved);
    } catch (e) {
      if (e instanceof ApiError && e.status === 422) {
        const known = new Set(inputs.map((i) => i.name));
        refused.current = e;
        for (const [field, messages] of Object.entries(e.fieldErrors)) {
          if (known.has(field)) form.setError(field, { type: "server", message: messages.join(" ") });
        }
        setFormErrors(() => () => {
          const other: string[] = [...e.formErrors];
          for (const [field, messages] of Object.entries(e.fieldErrors)) {
            if (!known.has(field)) other.push(`${field}: ${messages.join(" ")}`);
          }
          return other.length || Object.keys(e.fieldErrors).length ? other : [e.message];
        });
        focusFirstError();
      } else {
          setFormErrors(() => () => [e instanceof Error ? e.message : t("record.saveFailed")]);
      }
    }
  }, focusFirstError);

  // A retried create from this form that went through: continue as after a
  // normal save. Another form's write only gets the banner's notice.
  const recovered = async (result: unknown, pending: PendingWrite) => {
    const saved = (result as { data?: Envelope } | undefined)?.data;
    if (editing || !saved || pending.origin !== origin || !mounted.current) return false;
    await afterSave(saved);
    return true;
  };

  const title = editing
    ? t("record.editTitle", { record: record?.title ?? schema.resource.label.toLocaleLowerCase() })
    : t("list.new", { resource: schema.resource.label.toLocaleLowerCase() });
  // The language changed: same form and values, labels of the latest schema.
  const shown = relabel(components, schema.form.components);
  return (
    <Page className="max-w-4xl">
    <form className="flex min-w-0 flex-col gap-6" onSubmit={(e) => void submit(e)} noValidate>
      <PageHeader title={title} eyebrow={schema.resource.pluralLabel} description={editing ? t("record.editDescription") : t("record.createDescription")} />
      <VersionBanner schema={schema} />
      <PausedBanner scope={slug} onRecovered={recovered} />
      {refreshError instanceof Error && (
        <Alert variant="warning">{t("record.refreshFailed", { error: refreshError.message })}</Alert>
      )}
      {editing && !record && (
        <Alert>{t("record.blind")}</Alert>
      )}
      {formErrors().length > 0 && (
        <Alert variant="destructive">
          {formErrors().map((m) => (
            <p key={m}>{m}</p>
          ))}
        </Alert>
      )}
      <Card>
        <CardContent className={cn("grid grid-cols-1 gap-x-6 gap-y-6 p-5 sm:p-6", gridCols(schema.form.columns))}>
          <Components
            components={shown}
            ctx={{
              slug,
              record: recordKey,
              control: form.control,
              errors: form.formState.errors,
              // Locked while saving: edits made meanwhile would be lost on completion.
              // Also while paused: a retry re-sends the earlier submission, not later edits.
              readOnly: readOnly || save.isPending || uncertain,
              submitCount: form.formState.submitCount,
              onBusy,
              onUserChange: (name) => touched.current.add(name),
            }}
          />
        </CardContent>
      </Card>
      <div className="flex flex-wrap items-center gap-3 border-t border-border pt-5">
        <Button type="submit" disabled={readOnly || uncertain || busy.size > 0 || save.isPending || form.formState.isSubmitting}>
          {save.isPending ? t("record.saving") : editing ? t("record.saveChanges") : t("record.create")}
        </Button>
        <Link to="/r/$slug" params={{ slug }} className={linkClass}>
          {t("action.cancel")}
        </Link>
      </div>
    </form>
    </Page>
  );
}

export function ViewPage({ slug, recordKey }: { slug: string; recordKey: string }) {
  const t = useT();
  const navigate = useNavigate();
  // Completion redirects only apply while the page that started the write is shown.
  const mounted = useMounted();
  const schema = useSchema(slug, "view", recordKey);
  const record = useRecord(slug, recordKey);
  const del = useDeleteRecord(slug);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<Text | null>(null);
  const uncertain = useUncertain(slug);

  if (schema.isLoading || record.isLoading) return <Loading />;
  if (schema.error || !schema.data) return <ErrorState error={schema.error} />;
  if (record.error || !record.data) return <ErrorState error={record.error} />;
  const s = schema.data;
  const readOnly = compatibility(s).readOnly || uncertain;
  const remove = async () => {
    setConfirming(false);
    try {
      await del.mutateAsync(recordKey);
      if (mounted.current) await navigate({ to: "/r/$slug", params: { slug } });
    } catch (e) {
      setError(() => errorText(e, "list.deleteFailed"));
    }
  };

  return (
    <Page className="max-w-4xl">
      <PageHeader
        title={record.data.title ?? `${s.resource.label} ${recordKey}`}
        eyebrow={s.resource.pluralLabel}
        description={t("record.details")}
        actions={<>
          {record.data.abilities.update && !readOnly && (
            <Link to="/r/$slug/records/$key/edit" params={{ slug, key: recordKey }} className={linkClass}>
              <Pencil /> {t("action.edit")}
            </Link>
          )}
          {record.data.abilities.delete && !readOnly && (
            <Button variant="destructive" onClick={() => setConfirming(true)} disabled={del.isPending}>
              <Trash2 /> {t("action.delete")}
            </Button>
          )}
        </>}
      />
      <PausedBanner scope={slug} />
      <VersionBanner schema={s} />
      {error && <Alert variant="destructive">{show(error)}</Alert>}
      <Card>
        <CardContent className={cn("grid grid-cols-1 gap-x-6 gap-y-6 p-5 sm:p-6", gridCols(s.form.columns))}>
          <ReadOnlyComponents components={s.form.components} data={record.data.data} slug={slug} record={recordKey} />
        </CardContent>
      </Card>
      {(s.relations ?? []).map((relation) => (
        <RelationSection key={relation.name} parent={slug} record={recordKey} relation={relation} />
      ))}
      {s.abilities.history && <RecordHistory slug={slug} recordKey={recordKey} />}
      <Link to="/r/$slug" params={{ slug }} className={buttonVariants({ variant: "ghost", className: "self-start" })}>
        {t("record.back", { resource: s.resource.pluralLabel.toLocaleLowerCase() })}
      </Link>
      <Dialog open={confirming} onClose={() => setConfirming(false)} title={t("list.confirmDelete")}>
        <p className="text-sm text-muted-foreground">{t("record.cannotUndo")}</p>
        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => setConfirming(false)}>
            {t("action.cancel")}
          </Button>
          <Button variant="destructive" onClick={() => void remove()}>
            {t("action.delete")}
          </Button>
        </div>
      </Dialog>
    </Page>
  );
}
