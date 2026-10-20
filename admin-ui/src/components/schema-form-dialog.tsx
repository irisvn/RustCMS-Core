// A form from the server's schema in a dialog: action forms and the pivot
// columns of a link. The server checks every value again; its 422s land on
// the fields they name.
import { useMemo, useRef, useState, type ReactNode } from "react";
import { useForm } from "react-hook-form";
import { ApiError } from "@/api/client";
import type { FormSchema } from "@/api/types";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { useT } from "@/i18n";
import { Components } from "@/renderers/form";
import { type FormValues, defaultValues, inputsOf, payloadOf } from "@/schema/form";
import { editResolver } from "@/schema/resolver";

export function SchemaFormDialog(props: {
  open: boolean;
  onClose: () => void;
  title: string;
  form?: FormSchema;
  /** Resource of the inputs (option lookups and labels). */
  slug: string;
  /** Options endpoint of a relationship input, by name. */
  optionsPath?: (field: string) => string;
  /** Values to start from (pivot columns of a link). */
  data?: Record<string, unknown>;
  submitLabel: string;
  /** Sends the payload; throws to keep the dialog open (422s are shown on the fields). */
  onSubmit: (data: Record<string, unknown>) => Promise<void>;
  /** Shown above the inputs (e.g. the records to attach). */
  children?: ReactNode;
  /** Whether the payload may be sent (e.g. something is chosen). */
  ready?: boolean;
  /**
   * `action`: every input is sent (the server fills in defaults the same way).
   * `create` / `edit` (pivot columns of a link): only inputs the user changed,
   * so database defaults and values changed by others meanwhile are kept.
   */
  mode?: "action" | "create" | "edit";
}) {
  // Busy while submitting: closing is blocked until the answer arrives.
  const [busy, setBusy] = useState(false);
  // Mounted only while open: each opening starts from fresh values.
  return (
    <Dialog open={props.open} onClose={props.onClose} title={props.title} className="max-w-lg" closeDisabled={busy}>
      {props.open && <Body {...props} busy={busy} setBusy={setBusy} />}
    </Dialog>
  );
}

function Body({ onClose, form, slug, optionsPath, data, submitLabel, onSubmit, children, ready = true, mode = "action", busy, setBusy }: Parameters<typeof SchemaFormDialog>[0] & { busy: boolean; setBusy: (busy: boolean) => void }) {
  const t = useT();
  const components = useMemo(() => form?.components ?? [], [form]);
  const inputs = useMemo(() => inputsOf(components), [components]);
  const partial = mode !== "action";
  // Checked as submitted: all inputs of an action, the changed ones of a link.
  const dirty = useRef<(name: string) => boolean>(() => true);
  const resolver = useMemo(() => editResolver(inputs, partial, (name) => dirty.current(name)), [inputs, partial]);
  const values = useForm<FormValues>({ defaultValues: defaultValues(inputs, data), resolver });
  dirty.current = (name) => values.getFieldState(name).isDirty;
  const [notice, setNotice] = useState<string | null>(null);
  const submit = values.handleSubmit(async (current) => {
    setNotice(null);
    setBusy(true);
    try {
      const changed = partial ? Object.fromEntries(inputs.map((i) => [i.name, values.getFieldState(i.name).isDirty])) : undefined;
      await onSubmit(payloadOf(inputs, current, changed));
      onClose();
    } catch (e) {
      if (e instanceof ApiError && e.status === 422) {
        const known = new Set(inputs.map((i) => i.name));
        for (const [field, messages] of Object.entries(e.fieldErrors)) {
          if (known.has(field)) values.setError(field, { type: "server", message: messages.join(" ") });
        }
        const other = [...e.formErrors, ...Object.entries(e.fieldErrors).filter(([f]) => !known.has(f)).map(([f, m]) => `${f}: ${m.join(" ")}`)];
        if (other.length) setNotice(other.join(" "));
      } else {
        setNotice(e instanceof Error ? e.message : t("action.failed"));
      }
    } finally {
      setBusy(false);
    }
  });
  return (
    <form className="flex flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
      {children}
      {components.length > 0 && (
        <div className="grid gap-4">
          <Components components={components} ctx={{ slug, control: values.control, errors: values.formState.errors, optionsPath }} />
        </div>
      )}
      {notice && <Alert variant="destructive">{notice}</Alert>}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" onClick={onClose} disabled={busy}>
          {t("action.cancel")}
        </Button>
        <Button type="submit" disabled={busy || !ready}>
          {submitLabel}
        </Button>
      </div>
    </form>
  );
}
