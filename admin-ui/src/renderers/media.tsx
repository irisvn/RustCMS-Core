// Media inputs: upload to the media library, keep ids in the form. Files are
// shown only from the panel's own media URL (the CSP allows no `blob:`).
import { ArrowLeft, ArrowRight, FileText, Upload, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ApiError } from "@/api/client";
import { usePanelConfig, useSession } from "@/api/context";
import type { DataResponse, InputSchema, MediaItem } from "@/api/types";
import { Button } from "@/components/ui/button";
import { mediaUrl } from "@/lib/rich-text";
import { later, show, type Text, useT } from "@/i18n";

/**
 * Uploads `file` with the idempotency `key` (the same key on a retry: the
 * server stores it once). Returns the stored item.
 */
export async function uploadMedia(api: ReturnType<typeof useSession>["api"], file: File, key: string): Promise<MediaItem> {
  const form = new FormData();
  form.append("file", file, file.name);
  const response = await api.request<DataResponse<MediaItem>>("POST", "/media", form, { "Idempotency-Key": key });
  return response.data;
}

const accepts = (accept: string[], mime: string) =>
  accept.length === 0 || accept.some((a) => a === mime || (a.endsWith("/*") && mime.startsWith(a.slice(0, -1))));

/** A stored media item: an image preview, or a file link. */
export function MediaThumb({ id, item }: { id: string; item?: MediaItem }) {
  const t = useT();
  const { apiPath } = usePanelConfig();
  const [broken, setBroken] = useState(false);
  const url = mediaUrl(apiPath, id);
  if ((item && !item.inline) || broken) {
    return (
      <a href={url} className="inline-flex items-center gap-2 rounded-md border border-border px-2 py-1 text-sm hover:bg-muted">
        <FileText className="size-4" aria-hidden="true" />
        {item?.name ?? t("media.file")}
      </a>
    );
  }
  return <img src={url} alt={item?.name ?? ""} className="size-20 rounded-md border border-border object-cover" onError={() => setBroken(true)} />;
}

export function FileUploadWidget({ id, input, value, onChange, disabled, invalid, describedBy, onBusy }: {
  id: string;
  input: InputSchema;
  value: string | boolean | string[];
  onChange: (value: string | string[]) => void;
  disabled?: boolean;
  invalid: boolean;
  describedBy?: string;
  onBusy?: (busy: boolean) => void;
}) {
  const t = useT();
  const session = useSession();
  const picker = useRef<HTMLInputElement>(null);
  const rules = input.media ?? { multiple: false, maxItems: 1, accept: [] };
  const ids = Array.isArray(value) ? value : typeof value === "string" && value ? [value] : [];
  const [items, setItems] = useState<Record<string, MediaItem>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Text | null>(null);
  // An upload whose outcome is unknown: retried with the same file and key.
  const [retry, setRetry] = useState<{ file: File; key: string } | null>(null);
  const set = (next: string[]) => onChange(rules.multiple ? next : (next[0] ?? ""));
  // The latest value: an upload finishing later appends to it, not to the
  // value it started from (items removed meanwhile stay removed).
  const latest = useRef(ids);
  latest.current = ids;
  const mounted = useRef(true);
  useEffect(() => () => {
    mounted.current = false;
  }, []);
  // The form may not save while an upload or its retry is pending.
  useEffect(() => {
    onBusy?.(busy || retry !== null);
  }, [busy, retry, onBusy]);
  useEffect(() => () => onBusy?.(false), [onBusy]);

  const send = async (file: File, key: string) => {
    setBusy(true);
    setError(null);
    try {
      const item = await uploadMedia(session.api, file, key);
      if (!mounted.current) return;
      setRetry(null);
      setItems((all) => ({ ...all, [item.id]: item }));
      const current = latest.current.filter((i) => i !== item.id);
      set(rules.multiple ? [...current, item.id].slice(0, rules.maxItems) : [item.id]);
    } catch (e) {
      if (!mounted.current) return;
      if (e instanceof ApiError && e.maybeApplied) setRetry({ file, key });
      setError(() => () => (e instanceof ApiError && e.status === 422 ? Object.values(e.fieldErrors).flat().join(" ") || e.message : e instanceof Error ? e.message : t("media.uploadFailed")));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };
  const move = (from: number, to: number) => {
    const next = [...ids];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved!);
    set(next);
  };
  const pick = (file: File | undefined) => {
    if (!file) return;
    if (!accepts(rules.accept, file.type)) return setError(() => later("media.notAccepted", { file: file.name }));
    if (rules.maxSize && file.size > rules.maxSize) return setError(() => later("media.tooLarge", { file: file.name }));
    void send(file, crypto.randomUUID());
  };
  const full = ids.length >= rules.maxItems;
  return (
    <div id={id} tabIndex={-1} role="group" aria-label={input.label} aria-invalid={invalid || undefined} aria-describedby={describedBy} className="flex flex-col gap-3 focus-visible:outline-none">
      {ids.length > 0 && (
        <ul className="flex flex-wrap gap-3">
          {ids.map((mediaId, index) => (
            <li key={mediaId} className="flex items-start gap-1">
              <MediaThumb id={mediaId} item={items[mediaId]} />
              {!disabled && !busy && (
                <div className="flex flex-col">
                  <Button size="icon" variant="ghost" aria-label={t("media.remove", { n: index + 1 })} onClick={() => set(ids.filter((i) => i !== mediaId))}>
                    <X />
                  </Button>
                  {rules.multiple && index > 0 && (
                    <Button size="icon" variant="ghost" aria-label={t("media.earlier", { n: index + 1 })} onClick={() => move(index, index - 1)}>
                      <ArrowLeft />
                    </Button>
                  )}
                  {rules.multiple && index < ids.length - 1 && (
                    <Button size="icon" variant="ghost" aria-label={t("media.later", { n: index + 1 })} onClick={() => move(index, index + 1)}>
                      <ArrowRight />
                    </Button>
                  )}
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
      {ids.length === 0 && <p className="text-sm text-muted-foreground">{t("media.none")}</p>}
      <input ref={picker} type="file" hidden accept={rules.accept.join(",") || undefined} onChange={(e) => {
        pick(e.target.files?.[0]);
        e.target.value = "";
      }} />
      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" disabled={disabled || busy || (full && rules.multiple)} onClick={() => picker.current?.click()}>
          <Upload /> {busy ? t("media.uploading") : ids.length && !rules.multiple ? t("media.replace") : t("media.upload")}
        </Button>
        {retry && (
          <Button size="sm" disabled={disabled || busy} onClick={() => void send(retry.file, retry.key)}>
            {t("media.retry")}
          </Button>
        )}
      </div>
      {error && <p role="alert" className="text-xs text-destructive">{show(error)}</p>}
    </div>
  );
}
