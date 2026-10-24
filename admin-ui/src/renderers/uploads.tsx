// Uploads to the media library, several at a time: a queue that sends at most
// `max_concurrent_uploads` files at once, and never more than two (the limit is
// shared by every user: the server answers 429 beyond), each file with its own
// idempotency key (kept for its retries), status and error. A 429 is sent again
// by itself after `Retry-After` (the server refused it before reading it, and
// the key is the same), a few times, then shown as any failure. Files are
// checked against the server's size limit before sending; the server decides
// the type from the content.
import { Loader2, RotateCcw, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError } from "@/api/client";
import { useSession } from "@/api/context";
import { usePanel } from "@/api/queries";
import type { DataResponse, MediaItem } from "@/api/types";
import { Button } from "@/components/ui/button";
import { later, show, type Text, t, useT } from "@/i18n";

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

/** The message of a failed upload: the server's field messages for a refused file. */
export function uploadError(e: unknown): Text {
  return () => (e instanceof ApiError && e.status === 422 ? Object.values(e.fieldErrors).flat().join(" ") || e.message : e instanceof Error ? e.message : t("media.uploadFailed"));
}

export type UploadStatus = "queued" | "uploading" | "done" | "failed";

export interface Upload {
  /** Local id (React key). */
  id: string;
  file: File;
  /** Idempotency key: the same for every attempt of this file. */
  key: string;
  status: UploadStatus;
  error?: Text;
  /** A failure worth retrying (outcome unknown, busy server, network). */
  retryable: boolean;
  /** Its outcome is unknown: the form waits until it is retried or dismissed. */
  uncertain: boolean;
  /** Queued after a 429: sent again by itself in this many seconds (not before). */
  waiting?: number;
  /** How many times it was sent again by itself after a 429 (a retry by the user starts over). */
  autoRetries: number;
  item?: MediaItem;
  /** The session epoch it was added in: never sent in another one. */
  epoch: number;
}

/** Most files sent at once by one queue, whatever the server allows (its limit is shared). */
const QUEUE_CONCURRENCY = 2;
/** How many times a file refused with 429 is sent again by itself. */
const AUTO_RETRIES = 3;
/** Seconds to wait after a 429 without `Retry-After`. */
const DEFAULT_RETRY_AFTER = 5;

/** Whether a failure may succeed when sent again (with the same key). */
const retryable = (e: unknown) => !(e instanceof ApiError) || e.maybeApplied || e.status === 429 || e.status === 408 || e.status >= 500 || e.status === 0;

/**
 * A queue of uploads. `onUploaded` receives stored items in the order their
 * files were added (a file still uploading holds back the ones after it; a
 * failed one does not), only while the component is mounted.
 * `check` may refuse a file before it is sent (returns the message).
 */
export function useUploads({ onUploaded, check }: { onUploaded: (item: MediaItem, file: File) => void; check?: (file: File) => Text | null }) {
  const session = useSession();
  const panel = usePanel();
  const limits = panel.data?.uploads;
  // Unknown limits (panel still loading): one at a time.
  const concurrency = Math.max(1, Math.min(limits?.maxConcurrent ?? 1, QUEUE_CONCURRENCY));
  const maxBytes = limits?.maxBytes;
  const [uploads, setUploads] = useState<Upload[]>([]);
  const mounted = useRef(true);
  // Automatic retries waiting for their time: cancelled when the component goes.
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());
  useEffect(() => {
    mounted.current = true;
    const pending = timers.current;
    return () => {
      mounted.current = false;
      for (const timer of pending) clearTimeout(timer);
      pending.clear();
    };
  }, []);
  const deliver = useRef(onUploaded);
  deliver.current = onUploaded;
  const delivered = useRef(new Set<string>());

  const patch = useCallback((id: string, change: Partial<Upload>) => {
    if (mounted.current) setUploads((all) => all.map((u) => (u.id === id ? { ...u, ...change } : u)));
  }, []);

  // A new session (sign-out, another account): files not sent yet are not sent.
  useEffect(() => session.api.onEpochChange(() => {
    if (!mounted.current) return;
    setUploads((all) => all.map((u) => (u.status === "queued" ? { ...u, status: "failed", error: later("error.sessionChanged"), retryable: false, waiting: undefined } : u)));
  }), [session.api]);

  // After a 429: queued again, sent once `seconds` have passed (if still queued).
  const retryLater = useCallback((id: string, seconds: number, autoRetries: number) => {
    patch(id, { status: "queued", waiting: seconds, autoRetries });
    const timer = setTimeout(() => {
      timers.current.delete(timer);
      if (mounted.current) setUploads((all) => all.map((u) => (u.id === id && u.status === "queued" ? { ...u, waiting: undefined } : u)));
    }, seconds * 1000);
    timers.current.add(timer);
  }, [patch]);

  // Start queued files while there is room.
  useEffect(() => {
    const running = uploads.filter((u) => u.status === "uploading").length;
    const next = uploads.filter((u) => u.status === "queued" && u.waiting === undefined).slice(0, Math.max(0, concurrency - running));
    for (const upload of next) {
      if (upload.epoch !== session.api.epoch) {
        patch(upload.id, { status: "failed", error: later("error.sessionChanged"), retryable: false });
        continue;
      }
      patch(upload.id, { status: "uploading", error: undefined });
      uploadMedia(session.api, upload.file, upload.key).then(
        (item) => patch(upload.id, { status: "done", item, retryable: false, uncertain: false }),
        (e: unknown) => {
          // Gone meanwhile: nothing to show, nothing to send again.
          if (!mounted.current) return;
          // The server is busy (refused before reading the file): sent again by itself, the same key.
          if (e instanceof ApiError && e.status === 429 && !e.maybeApplied && upload.autoRetries < AUTO_RETRIES) {
            return retryLater(upload.id, e.body.retryAfter ?? DEFAULT_RETRY_AFTER, upload.autoRetries + 1);
          }
          patch(upload.id, {
            status: "failed",
            error: uploadError(e),
            retryable: retryable(e),
            uncertain: e instanceof ApiError && e.maybeApplied,
          });
        },
      );
    }
  }, [uploads, concurrency, session.api, patch, retryLater]);

  // Hand over finished files in order, then forget them.
  useEffect(() => {
    const ready: Upload[] = [];
    for (const u of uploads) {
      if (u.status === "queued" || u.status === "uploading") break;
      if (u.status === "done" && !delivered.current.has(u.id)) ready.push(u);
    }
    if (ready.length === 0) return;
    for (const u of ready) {
      delivered.current.add(u.id);
      deliver.current(u.item!, u.file);
    }
    setUploads((all) => all.filter((u) => !delivered.current.has(u.id)));
  }, [uploads]);

  const add = useCallback((files: Iterable<File>) => {
    const added: Upload[] = [];
    for (const file of files) {
      const base = { id: crypto.randomUUID(), file, key: crypto.randomUUID(), retryable: false, uncertain: false, autoRetries: 0, epoch: session.api.epoch };
      const refused = check?.(file) ?? (maxBytes !== undefined && file.size > maxBytes ? later("media.tooLarge", { file: file.name }) : null);
      added.push(refused ? { ...base, status: "failed", error: refused } : { ...base, status: "queued" });
    }
    if (added.length) setUploads((all) => [...all, ...added]);
  }, [check, maxBytes, session.api]);
  // A retry belongs to the current session (the same key: the server stores it once).
  const retry = useCallback((id: string) => patch(id, { status: "queued", error: undefined, autoRetries: 0, epoch: session.api.epoch }), [patch, session.api]);
  const dismiss = useCallback((id: string) => setUploads((all) => all.filter((u) => u.id !== id)), []);
  const busy = uploads.some((u) => u.status === "queued" || u.status === "uploading" || u.uncertain);
  return { uploads, add, retry, dismiss, busy };
}

/** Per-file status of a queue: waiting, sending, or failed with its message, a retry and a dismiss. */
export function UploadList({ uploads, onRetry, onDismiss, disabled }: {
  uploads: Upload[];
  onRetry: (id: string) => void;
  onDismiss: (id: string) => void;
  disabled?: boolean;
}) {
  const t = useT();
  if (uploads.length === 0) return null;
  return (
    <ul aria-label={t("upload.list")} className="mt-2 flex flex-col gap-1">
      {uploads.map((u) => (
        <li key={u.id} className="flex flex-wrap items-center gap-2 rounded-md border border-border px-2 py-1 text-xs">
          {u.status === "uploading" && <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />}
          <span className="min-w-0 flex-1 truncate font-medium">{u.file.name}</span>
          {u.status === "failed"
            ? <span role="alert" className="text-destructive">{u.error ? show(u.error) : t("media.uploadFailed")}</span>
            : (
              <>
                <span className="text-muted-foreground">
                  {u.waiting !== undefined ? t("upload.busy", { seconds: u.waiting }) : u.status === "queued" ? t("upload.queued") : t("upload.sending")}
                </span>
                {/* Native, without a value: indeterminate (fetch reports no upload progress). */}
                <progress className="h-1.5 w-20" aria-label={t("upload.progress", { file: u.file.name })} />
              </>
            )}
          {u.status === "failed" && u.retryable && (
            <Button size="sm" variant="outline" disabled={disabled} aria-label={t("upload.retry", { file: u.file.name })} onClick={() => onRetry(u.id)}>
              <RotateCcw /> {t("media.retry")}
            </Button>
          )}
          {u.status === "failed" && (
            <Button size="icon" variant="ghost" aria-label={t("upload.dismiss", { file: u.file.name })} onClick={() => onDismiss(u.id)}>
              <X />
            </Button>
          )}
        </li>
      ))}
    </ul>
  );
}
