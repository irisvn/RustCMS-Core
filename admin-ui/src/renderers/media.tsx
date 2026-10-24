// Media inputs: upload to the media library, keep ids in the form. Files are
// shown only from the panel's own media URL (the CSP allows no `blob:`).
import { ArrowLeft, ArrowRight, FileText, ImageOff, Library, Upload, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ApiError } from "@/api/client";
import { usePanelConfig, useSession } from "@/api/context";
import type { InputSchema, MediaItem } from "@/api/types";
import { Button } from "@/components/ui/button";
import { mediaUrl } from "@/lib/rich-text";
import { later, show, type Text, useT } from "@/i18n";
import { MediaPicker, accepts, usePickerAccess } from "./media-picker";
import { uploadMedia } from "./uploads";

export { uploadMedia };

/** A file that cannot be shown (missing, trashed, not visible to the user). */
export function MediaUnavailable() {
  const t = useT();
  return (
    <span role="img" aria-label={t("media.unavailable")} className="inline-flex items-center gap-2 rounded-md border border-dashed border-border px-2 py-1 text-sm text-muted-foreground">
      <ImageOff className="size-4" aria-hidden="true" />
      {t("media.unavailable")}
    </span>
  );
}

/** An image of the media library (`src`: its panel URL), or the "unavailable" chip when it cannot be loaded. */
export function MediaImage({ src, alt, className }: { src: string; alt: string; className?: string }) {
  // Failed for this source only: another source is tried again.
  const [failed, setFailed] = useState<string | null>(null);
  if (failed === src) return <MediaUnavailable />;
  return <img src={src} alt={alt} className={className} onError={() => setFailed(src)} />;
}

/** A stored media item: an image preview, or a file link ("unavailable" when it cannot be fetched). */
export function MediaThumb({ id, item }: { id: string; item?: MediaItem }) {
  const t = useT();
  const { apiPath } = usePanelConfig();
  const url = mediaUrl(apiPath, id);
  // Not shown as an image: a file of another type (PDF), or not available. The server says which.
  const [state, setState] = useState<{ url: string; kind: "file" | "unavailable" } | null>(null);
  const shown = state?.url === url ? state.kind : null;
  const probe = () => {
    void fetch(url, { method: "HEAD", credentials: "same-origin", cache: "no-store" })
      .then((r) => setState({ url, kind: r.ok ? "file" : "unavailable" }))
      .catch(() => setState({ url, kind: "unavailable" }));
  };
  if (shown === "unavailable") return <MediaUnavailable />;
  if ((item && !item.inline) || shown === "file") {
    return (
      <a href={url} className="inline-flex items-center gap-2 rounded-md border border-border px-2 py-1 text-sm hover:bg-muted">
        <FileText className="size-4" aria-hidden="true" />
        {item?.name ?? t("media.file")}
      </a>
    );
  }
  return <img src={url} alt={item?.name ?? ""} className="size-20 rounded-md border border-border object-cover" onError={probe} />;
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
  const access = usePickerAccess();
  const [picking, setPicking] = useState(false);
  const set = (next: string[]) => onChange(rules.multiple ? next : (next[0] ?? ""));
  // The latest value: an upload finishing later appends to it, not to the
  // value it started from (items removed meanwhile stay removed).
  const latest = useRef(ids);
  latest.current = ids;
  const mounted = useRef(true);
  useEffect(() => () => {
    mounted.current = false;
  }, []);
  // The form may not save while an upload or its retry is pending. The
  // callback may be a new function on every render: only state changes report.
  const reportBusy = useRef(onBusy);
  reportBusy.current = onBusy;
  useEffect(() => {
    reportBusy.current?.(busy || retry !== null);
  }, [busy, retry]);
  useEffect(() => () => reportBusy.current?.(false), []);

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
        {access.library && (
          <Button size="sm" variant="outline" disabled={disabled || busy || (full && rules.multiple)} onClick={() => setPicking(true)}>
            <Library /> {t("picker.open")}
          </Button>
        )}
        {retry && (
          <Button size="sm" disabled={disabled || busy} onClick={() => void send(retry.file, retry.key)}>
            {t("media.retry")}
          </Button>
        )}
      </div>
      {error && <p role="alert" className="text-xs text-destructive">{show(error)}</p>}
      <MediaPicker
        open={picking}
        title={input.label}
        action={t("picker.use")}
        rules={{ multiple: rules.multiple, max: rules.multiple ? rules.maxItems - ids.length : 1, accept: rules.accept, maxSize: rules.maxSize, exclude: ids }}
        onClose={() => setPicking(false)}
        onPick={(picked) => {
          setPicking(false);
          setItems((all) => ({ ...all, ...Object.fromEntries(picked.map((item) => [item.id, item])) }));
          const fresh = picked.map((item) => item.id).filter((pickedId) => !latest.current.includes(pickedId));
          set(rules.multiple ? [...latest.current, ...fresh].slice(0, rules.maxItems) : [picked[0]!.id]);
        }}
      />
    </div>
  );
}
