import { Link, useNavigate } from "@tanstack/react-router";
import { Pencil, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { ApiError } from "@/api/client";
import { useUncertain } from "@/api/context";
import { useDeleteRecord, useRecord, useSaveRecord, useSchema } from "@/api/queries";
import type { Envelope, InputSchema, ResourceSchema } from "@/api/types";
import { ErrorState, Loading, VersionBanner, compatibility } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Button, buttonVariants } from "@/components/ui/button";
import { Page, PageHeader } from "@/components/layout";
import { Card, CardContent } from "@/components/ui/card";
import { Dialog } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { Components, ReadOnlyComponents, gridCols } from "@/renderers/form";
import { type FormValues, defaultValues, inputsOf, payloadOf, protectLossyJson } from "@/schema/form";
import { editResolver } from "@/schema/resolver";


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
export function FormPage({ slug, recordKey }: { slug: string; recordKey?: string }) {
  const editing = recordKey !== undefined;
  const schema = useSchema(slug, editing ? "edit" : "create", recordKey);
  // An editor may update without viewing: then the form starts empty (only changed fields are sent).
  // A blind form, once shown, stays blind (and mounted) if a refetch grants `view`:
  // loading the record would replace the page and lose the draft.
  const identity = `${slug}:${recordKey ?? ""}`;
  const shownBlind = useRef<string | null>(null);
  const viewable = !editing || (schema.data?.abilities.view === true && shownBlind.current !== identity);
  const record = useRecord(slug, recordKey ?? "", { enabled: editing && viewable && !!schema.data });

  if (schema.isLoading || (editing && viewable && record.isLoading)) return <Loading />;
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
}: {
  slug: string;
  recordKey?: string;
  schema: ResourceSchema;
  record?: Envelope;
  refreshError?: unknown;
}) {
  const navigate = useNavigate();
  // Completion redirects only apply while the page that started the write is shown.
  const mounted = useMounted();
  const editing = recordKey !== undefined;
  // Fixed at mount, like the defaults: JSON values that lost precision on the way here stay read-only.
  const [components] = useState(() => protectLossyJson(schema.form.components, record?.data));
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
  const save = useSaveRecord(slug, recordKey);
  const [formErrors, setFormErrors] = useState<string[]>([]);
  // After a write with an unknown outcome, resubmitting could create a duplicate.
  const uncertain = useUncertain(slug);

  const submit = form.handleSubmit(async (values) => {
    setFormErrors([]);
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
      if (!mounted.current) return;
      if (saved.abilities.view) {
        await navigate({ to: "/r/$slug/records/$key", params: { slug, key: saved.key } });
      } else {
        await navigate({ to: "/r/$slug", params: { slug } });
      }
    } catch (e) {
      if (e instanceof ApiError && e.status === 422) {
        const known = new Set(inputs.map((i) => i.name));
        const other: string[] = [...e.formErrors];
        for (const [field, messages] of Object.entries(e.fieldErrors)) {
          if (known.has(field)) form.setError(field, { message: messages.join(" ") });
          else other.push(`${field}: ${messages.join(" ")}`);
        }
        setFormErrors(other.length || Object.keys(e.fieldErrors).length ? other : [e.message]);
      } else {
          setFormErrors([e instanceof Error ? e.message : "Could not save."]);
      }
    }
  });

  const title = editing ? `Edit ${record?.title ?? schema.resource.label.toLowerCase()}` : `New ${schema.resource.label.toLowerCase()}`;
  return (
    <Page className="max-w-4xl">
    <form className="flex min-w-0 flex-col gap-6" onSubmit={(e) => void submit(e)} noValidate>
      <PageHeader title={title} eyebrow={schema.resource.pluralLabel} description={editing ? "Update the record details below." : "Fill in the details to create a record."} />
      <VersionBanner schema={schema} />
      {refreshError instanceof Error && (
        <Alert variant="warning">Could not refresh this page ({refreshError.message}). Your changes are kept.</Alert>
      )}
      {editing && !record && (
        <Alert>You may update this record but not view it: only the fields you fill in are changed.</Alert>
      )}
      {formErrors.length > 0 && (
        <Alert variant="destructive">
          {formErrors.map((m) => (
            <p key={m}>{m}</p>
          ))}
        </Alert>
      )}
      <Card>
        <CardContent className={cn("grid grid-cols-1 gap-x-6 gap-y-6 p-5 sm:p-6", gridCols(schema.form.columns))}>
          <Components
            components={components}
            ctx={{
              slug,
              record: recordKey,
              control: form.control,
              errors: form.formState.errors,
              // Locked while saving: edits made meanwhile would be lost on completion.
              readOnly: readOnly || save.isPending,
              onUserChange: (name) => touched.current.add(name),
            }}
          />
        </CardContent>
      </Card>
      <div className="flex flex-wrap items-center gap-3 border-t border-border pt-5">
        <Button type="submit" disabled={readOnly || uncertain || save.isPending || form.formState.isSubmitting}>
          {save.isPending ? "Saving…" : editing ? "Save changes" : "Create"}
        </Button>
        <Link to="/r/$slug" params={{ slug }} className={linkClass}>
          Cancel
        </Link>
      </div>
    </form>
    </Page>
  );
}

export function ViewPage({ slug, recordKey }: { slug: string; recordKey: string }) {
  const navigate = useNavigate();
  // Completion redirects only apply while the page that started the write is shown.
  const mounted = useMounted();
  const schema = useSchema(slug, "view", recordKey);
  const record = useRecord(slug, recordKey);
  const del = useDeleteRecord(slug);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
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
      setError(e instanceof Error ? e.message : "Could not delete.");
    }
  };

  return (
    <Page className="max-w-4xl">
      <PageHeader
        title={record.data.title ?? `${s.resource.label} ${recordKey}`}
        eyebrow={s.resource.pluralLabel}
        description="Record details"
        actions={<>
          {record.data.abilities.update && !readOnly && (
            <Link to="/r/$slug/records/$key/edit" params={{ slug, key: recordKey }} className={linkClass}>
              <Pencil /> Edit
            </Link>
          )}
          {record.data.abilities.delete && !readOnly && (
            <Button variant="destructive" onClick={() => setConfirming(true)} disabled={del.isPending}>
              <Trash2 /> Delete
            </Button>
          )}
        </>}
      />
      <VersionBanner schema={s} />
      {error && <Alert variant="destructive">{error}</Alert>}
      <Card>
        <CardContent className={cn("grid grid-cols-1 gap-x-6 gap-y-6 p-5 sm:p-6", gridCols(s.form.columns))}>
          <ReadOnlyComponents components={s.form.components} data={record.data.data} slug={slug} record={recordKey} />
        </CardContent>
      </Card>
      <Link to="/r/$slug" params={{ slug }} className={buttonVariants({ variant: "ghost", className: "self-start" })}>
        ← Back to {s.resource.pluralLabel.toLowerCase()}
      </Link>
      <Dialog open={confirming} onClose={() => setConfirming(false)} title="Delete record?">
        <p className="text-sm text-muted-foreground">This cannot be undone.</p>
        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => setConfirming(false)}>
            Cancel
          </Button>
          <Button variant="destructive" onClick={() => void remove()}>
            Delete
          </Button>
        </div>
      </Dialog>
    </Page>
  );
}
