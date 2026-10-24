// The Media resource page: a grid of thumbnails (or the usual table, the
// user's choice in this browser) and an upload queue (several files, one at a
// time, each with its state, error and retry).
import { useQueryClient } from "@tanstack/react-query";
import { FileText, LayoutGrid, List, RotateCcw, Upload, X } from "lucide-react";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { ApiError } from "@/api/client";
import { usePanelConfig, useSession } from "@/api/context";
import { invalidateAfterWrite } from "@/api/queries";
import type { Envelope } from "@/api/types";
import { Button } from "@/components/ui/button";
import { mediaUrl } from "@/lib/rich-text";
import { cn } from "@/lib/utils";
import { errorText, formatCount, show, type Text, useT } from "@/i18n";
import { uploadMedia } from "./media";

/** Slug of the media library resource (`rustcms_admin::media::SLUG`). */
export const MEDIA_SLUG = "media";

export type MediaView = "grid" | "list";

const viewKey = (user: string | null) => `rustcms.mediaView.${user ?? "anonymous"}`;

function storedView(user: string | null): MediaView {
  try {
    return window.localStorage.getItem(viewKey(user)) === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
}

/** Grid or list, remembered per user in this browser (grid by default). */
export function useMediaView(): [MediaView, (view: MediaView) => void] {
  const user = useSession().user();
  const [state, setState] = useState<{ user: string | null; view: MediaView }>(() => ({ user, view: storedView(user) }));
  // Another account in this tab: its own choice.
  const view = state.user === user ? state.view : storedView(user);
  const choose = useCallback((next: MediaView) => {
    setState({ user, view: next });
    try {
      window.localStorage.setItem(viewKey(user), next);
    } catch {
      // Blocked storage: this page only.
    }
  }, [user]);
  return [view, choose];
}

export function MediaViewToggle({ view, onChange }: { view: MediaView; onChange: (view: MediaView) => void }) {
  const t = useT();
  return (
    <div role="group" aria-label={t("mediaLibrary.view")} className="inline-flex rounded-md border border-border">
      <Button size="icon" variant={view === "grid" ? "secondary" : "ghost"} aria-pressed={view === "grid"} aria-label={t("mediaLibrary.grid")} onClick={() => onChange("grid")}>
        <LayoutGrid />
      </Button>
      <Button size="icon" variant={view === "list" ? "secondary" : "ghost"} aria-pressed={view === "list"} aria-label={t("mediaLibrary.list")} onClick={() => onChange("list")}>
        <List />
      </Button>
    </div>
  );
}

/** Whether the browser shows the file inline (the server serves these types `inline`). */
export const isImage = (mime: unknown) => typeof mime === "string" && ["image/png", "image/jpeg", "image/gif", "image/webp"].includes(mime);

/** A record's preview: the file itself, scaled by the browser, or a file icon. */
export function GridThumb({ record }: { record: Envelope }) {
  const { apiPath } = usePanelConfig();
  const [broken, setBroken] = useState(false);
  const alt = typeof record.data.alt === "string" && record.data.alt ? record.data.alt : (record.title ?? "");
  if (record.trashed || broken || !isImage(record.data.mime)) {
    return (
      <div className="flex aspect-square w-full items-center justify-center rounded-md bg-muted text-muted-foreground">
        <FileText className="size-8" aria-hidden="true" />
      </div>
    );
  }
  return (
    <img src={mediaUrl(apiPath, record.key)} alt={alt} loading="lazy" decoding="async" onError={() => setBroken(true)}
      className="aspect-square w-full rounded-md bg-muted object-cover" />
  );
}

/** The page's records as cards; `select` and `actions` come from the list page. */
export function MediaGrid({ rows, select, actions, badge }: {
  rows: Envelope[];
  select?: (record: Envelope) => React.ReactNode;
  actions: (record: Envelope) => React.ReactNode;
  badge: (record: Envelope) => React.ReactNode;
}) {
  return (
    <ul className="grid grid-cols-2 gap-3 p-4 sm:grid-cols-3 sm:p-5 md:grid-cols-4 lg:grid-cols-6">
      {rows.map((record) => (
        <li key={record.key} className="flex min-w-0 flex-col gap-2 rounded-lg border border-border p-2">
          <GridThumb record={record} />
          <div className="flex min-w-0 items-start gap-2">
            {select?.(record)}
            <span className="min-w-0 flex-1 truncate text-sm" title={record.title ?? record.key}>{record.title ?? record.key}</span>
          </div>
          {badge(record)}
          <div className="mt-auto">{actions(record)}</div>
        </li>
      ))}
    </ul>
  );
}

type UploadState = "queued" | "uploading" | "done" | "failed";

interface QueuedFile {
  id: string;
  file: File;
  /** The idempotency key: the same on a retry, so the server stores the file once. */
  key: string;
  state: UploadState;
  error?: Text;
}

/** "Upload files": several files, sent one at a time; failed ones can be retried. */
export function MediaUploader({ disabled }: { disabled?: boolean }) {
  const t = useT();
  const session = useSession();
  const client = useQueryClient();
  const picker = useRef<HTMLInputElement>(null);
  // The queue lives in a ref (the uploading loop reads it between awaits); renders follow it.
  const store = useRef<QueuedFile[]>([]);
  const [, rerender] = useReducer((n: number) => n + 1, 0);
  const running = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const setQueue = useCallback((next: QueuedFile[]) => {
    store.current = next;
    if (mounted.current) rerender();
  }, []);
  const update = (id: string, change: Partial<QueuedFile>) =>
    setQueue(store.current.map((item) => (item.id === id ? { ...item, ...change } : item)));
  // The queue belongs to the session it was filled in.
  useEffect(() => session.api.onEpochChange(() => setQueue([])), [session.api, setQueue]);

  /** Sends queued files one at a time (the server bounds uploads per user). */
  const pump = async () => {
    if (running.current) return;
    running.current = true;
    try {
      for (;;) {
        const next = store.current.find((item) => item.state === "queued");
        if (!next || !mounted.current) return;
        update(next.id, { state: "uploading", error: undefined });
        try {
          await uploadMedia(session.api, next.file, next.key);
          update(next.id, { state: "done" });
          void invalidateAfterWrite(client, MEDIA_SLUG);
        } catch (e) {
          const error: Text = e instanceof ApiError && e.status === 422
            ? () => Object.values(e.fieldErrors).flat().join(" ") || e.message
            : errorText(e, "media.uploadFailed");
          update(next.id, { state: "failed", error });
        }
      }
    } finally {
      running.current = false;
    }
  };

  const add = (files: FileList | null) => {
    if (!files || files.length === 0) return;
    const added = [...files].map((file) => ({ id: crypto.randomUUID(), file, key: crypto.randomUUID(), state: "queued" as const }));
    setQueue([...store.current.filter((item) => item.state !== "done"), ...added]);
    void pump();
  };
  const retry = (id: string) => {
    update(id, { state: "queued", error: undefined });
    void pump();
  };
  const queue = store.current;
  const pending = queue.filter((item) => item.state === "queued" || item.state === "uploading").length;

  return (
    <div className="flex flex-col items-end gap-2">
      <input ref={picker} type="file" multiple hidden data-testid="media-upload-input" onChange={(e) => {
        add(e.target.files);
        e.target.value = "";
      }} />
      <Button disabled={disabled} onClick={() => picker.current?.click()}>
        <Upload /> {t("mediaLibrary.upload")}
      </Button>
      {queue.length > 0 && (
        <section aria-label={t("mediaLibrary.uploads")} className="w-full max-w-sm rounded-lg border border-border bg-card p-3 text-sm sm:w-80">
          <div className="mb-2 flex items-center justify-between gap-2">
            <span className="font-medium" aria-live="polite">
              {pending > 0 ? t("mediaLibrary.pending", { count: pending, n: pending }) : t("mediaLibrary.finished")}
            </span>
            {pending === 0 && (
              <Button size="icon" variant="ghost" aria-label={t("mediaLibrary.clear")} onClick={() => setQueue([])}>
                <X />
              </Button>
            )}
          </div>
          <ul className="flex flex-col gap-2">
            {queue.map((item) => (
              <li key={item.id} className="flex flex-col gap-1">
                <div className="flex items-center gap-2">
                  <span className="min-w-0 flex-1 truncate" title={item.file.name}>{item.file.name}</span>
                  <span className={cn("text-xs", item.state === "failed" ? "text-destructive" : "text-muted-foreground")}>
                    {t(`mediaLibrary.state.${item.state}`)}
                  </span>
                  {item.state === "failed" && (
                    <Button size="icon" variant="ghost" aria-label={t("mediaLibrary.retry", { file: item.file.name })} onClick={() => retry(item.id)}>
                      <RotateCcw />
                    </Button>
                  )}
                </div>
                <progress
                  className="h-1 w-full accent-primary"
                  aria-label={t("mediaLibrary.progress", { file: item.file.name })}
                  max={1}
                  // No value while uploading: the browser shows an indeterminate bar.
                  value={item.state === "done" ? 1 : item.state === "uploading" ? undefined : 0}
                />
                {item.error && <p role="alert" className="text-xs text-destructive">{show(item.error)}</p>}
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

/** The file on its record's page: the image (or a download link) and what was detected. */
export function MediaPreview({ record }: { record: Envelope }) {
  const t = useT();
  const { apiPath } = usePanelConfig();
  const [broken, setBroken] = useState(false);
  const url = mediaUrl(apiPath, record.key);
  const data = record.data;
  const name = typeof data.name === "string" ? data.name : (record.title ?? record.key);
  const details = [
    typeof data.mime === "string" ? data.mime : null,
    typeof data.size === "string" ? t("mediaLibrary.bytes", { n: formatCount(Number(data.size)) }) : null,
    typeof data.width === "number" && typeof data.height === "number" ? `${data.width} × ${data.height}` : null,
  ].filter((d): d is string => d !== null);
  return (
    <figure className="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
      {isImage(data.mime) && !broken && !record.trashed ? (
        <img src={url} alt={typeof data.alt === "string" ? data.alt : ""} onError={() => setBroken(true)}
          className="max-h-96 w-full rounded-md bg-muted object-contain" />
      ) : (
        <a href={url} className="inline-flex items-center gap-2 self-start rounded-md border border-border px-3 py-2 text-sm hover:bg-muted">
          <FileText className="size-4" aria-hidden="true" /> {name}
        </a>
      )}
      {details.length > 0 && <figcaption className="text-xs text-muted-foreground">{details.join(" · ")}</figcaption>}
    </figure>
  );
}
