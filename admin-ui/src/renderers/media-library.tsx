// The Media resource page: a grid of thumbnails (or the usual table, the
// user's choice in this browser) and an upload queue (several files, each with
// its state, error and retry).
import { useQueryClient } from "@tanstack/react-query";
import { FileText, LayoutGrid, List, Upload, X } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { usePanelConfig, useSession } from "@/api/context";
import { invalidateAfterWrite } from "@/api/queries";
import type { Envelope } from "@/api/types";
import { Button } from "@/components/ui/button";
import { mediaUrl } from "@/lib/rich-text";
import { formatCount, useT } from "@/i18n";
import { MediaImage, MediaUnavailable } from "./media";
import { PER_PAGE, isInlineImage } from "./media-picker";
import { UploadList, useUploads } from "./uploads";

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

/** Thumbnails a grid page holds at most (each loads the original file), as in the picker. */
export const GRID_PER_PAGE = PER_PAGE;

/** The page sizes offered in the grid: the list's own up to {@link GRID_PER_PAGE}, then that. */
export const gridPageSizes = (options: number[]) => [...options.filter((n) => n < GRID_PER_PAGE), GRID_PER_PAGE];

/** A record's preview: the file itself, scaled by the browser, or a file icon. */
export function GridThumb({ record, generation = 0 }: { record: Envelope; generation?: number }) {
  const { apiPath } = usePanelConfig();
  const src = mediaUrl(apiPath, record.key);
  // Failed for this source and this fetch of the list only: tried again
  // after the next fetch (`generation`: when the list's data last arrived,
  // even unchanged) or for another file.
  const [failed, setFailed] = useState<{ src: string; generation: number } | null>(null);
  const broken = failed?.src === src && failed.generation === generation;
  const alt = typeof record.data.alt === "string" && record.data.alt ? record.data.alt : (record.title ?? "");
  if (record.trashed || broken || !isInlineImage(record.data.mime)) {
    return (
      <div className="flex aspect-square w-full items-center justify-center rounded-md bg-muted text-muted-foreground">
        <FileText className="size-8" aria-hidden="true" />
      </div>
    );
  }
  return (
    <img src={src} alt={alt} loading="lazy" decoding="async" onError={() => setFailed({ src, generation })}
      className="aspect-square w-full rounded-md bg-muted object-cover" />
  );
}

/** The page's records as cards; `select` and `actions` come from the list page. */
export function MediaGrid({ rows, generation, select, actions, badge }: {
  rows: Envelope[];
  /** When the rows last arrived (`dataUpdatedAt`): failed thumbnails are tried again after a fetch. */
  generation?: number;
  select?: (record: Envelope) => React.ReactNode;
  actions: (record: Envelope) => React.ReactNode;
  badge: (record: Envelope) => React.ReactNode;
}) {
  return (
    <ul className="grid grid-cols-2 gap-3 p-4 sm:grid-cols-3 sm:p-5 md:grid-cols-4 lg:grid-cols-6">
      {/* At most a grid page, whatever is shown meanwhile (a table's larger page as placeholder). */}
      {rows.slice(0, GRID_PER_PAGE).map((record) => (
        <li key={record.key} className="flex min-w-0 flex-col gap-2 rounded-lg border border-border p-2">
          <GridThumb record={record} generation={generation} />
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

/**
 * "Upload files": the media library's queue ({@link useUploads}: the server's
 * size limit checked first, two at a time at most, a busy server waited for,
 * a retry only when it may succeed), the list refreshed after each file.
 */
export function MediaUploader({ disabled }: { disabled?: boolean }) {
  const t = useT();
  const client = useQueryClient();
  const picker = useRef<HTMLInputElement>(null);
  // Files stored since the list was last cleared (the queue forgets them once in).
  const [uploaded, setUploaded] = useState(0);
  const uploads = useUploads({
    onUploaded: () => {
      setUploaded((n) => n + 1);
      void invalidateAfterWrite(client, MEDIA_SLUG);
    },
  });
  const pending = uploads.uploads.filter((u) => u.status === "queued" || u.status === "uploading").length;
  const clear = () => {
    setUploaded(0);
    for (const u of uploads.uploads) uploads.dismiss(u.id);
  };

  return (
    <div className="flex flex-col items-end gap-2">
      <input ref={picker} type="file" multiple hidden data-testid="media-upload-input" onChange={(e) => {
        uploads.add([...(e.target.files ?? [])]);
        e.target.value = "";
      }} />
      <Button disabled={disabled} onClick={() => picker.current?.click()}>
        <Upload /> {t("mediaLibrary.upload")}
      </Button>
      {(uploads.uploads.length > 0 || uploaded > 0) && (
        <section aria-label={t("mediaLibrary.uploads")} className="w-full max-w-sm rounded-lg border border-border bg-card p-3 text-sm sm:w-80">
          <div className="flex items-center justify-between gap-2">
            <span className="font-medium" aria-live="polite">
              {pending > 0 ? t("mediaLibrary.pending", { count: pending, n: pending }) : t("mediaLibrary.finished")}
            </span>
            {pending === 0 && (
              <Button size="icon" variant="ghost" aria-label={t("mediaLibrary.clear")} onClick={clear}>
                <X />
              </Button>
            )}
          </div>
          {uploaded > 0 && <p className="text-xs text-muted-foreground">{t("mediaLibrary.uploaded", { count: uploaded, n: uploaded })}</p>}
          <UploadList uploads={uploads.uploads} onRetry={uploads.retry} onDismiss={uploads.dismiss} disabled={disabled} />
        </section>
      )}
    </div>
  );
}

/** The file on its record's page: the image (or a download link) and what was detected. */
export function MediaPreview({ record }: { record: Envelope }) {
  const t = useT();
  const { apiPath } = usePanelConfig();
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
      {/* A trashed file is not served: nothing to show or download. */}
      {record.trashed ? (
        <span className="self-start"><MediaUnavailable /></span>
      ) : isInlineImage(data.mime) ? (
        <MediaImage src={url} alt={typeof data.alt === "string" ? data.alt : ""} className="max-h-96 w-full rounded-md bg-muted object-contain" />
      ) : (
        <a href={url} className="inline-flex items-center gap-2 self-start rounded-md border border-border px-3 py-2 text-sm hover:bg-muted">
          <FileText className="size-4" aria-hidden="true" /> {name}
        </a>
      )}
      {details.length > 0 && <figcaption className="text-xs text-muted-foreground">{details.join(" · ")}</figcaption>}
    </figure>
  );
}
