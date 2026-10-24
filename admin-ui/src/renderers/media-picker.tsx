// The media library picker: a dialog with the library (thumbnails, search,
// pages) and an upload tab, shared by the rich text editor and the media
// inputs. The library lists what the server's scope shows the user (own
// uploads, or every file with `media.view_all`); the server checks every
// reference again on save. Thumbnails are the authorized file URL, scaled by
// the browser (no server resizing); nothing is loaded from elsewhere.
import { useQueryClient } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, FileText, Upload as UploadIcon } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { usePanelConfig } from "@/api/context";
import { keys, usePanel, useRecords } from "@/api/queries";
import type { Envelope, MediaItem } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input, Label } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mediaUrl } from "@/lib/rich-text";
import { cn } from "@/lib/utils";
import { later, type Text, useT } from "@/i18n";
import { UploadList, useUploads } from "./uploads";

/** Types the media library stores (`rustcms_admin::media`), and those shown inline. */
const TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp", "application/pdf"];
const INLINE = new Set(["image/png", "image/jpeg", "image/gif", "image/webp"]);
const PER_PAGE = 24;

/** Whether `mime` matches an `accept` list (`image/*`; empty accepts all). */
export const accepts = (accept: string[], mime: string) =>
  accept.length === 0 || accept.some((a) => a === mime || (a.endsWith("/*") && mime.startsWith(a.slice(0, -1))));

/** The `mime` filter for an `accept` list: the stored types it matches (null: no filter). */
export function mimeFilter(accept: string[]): string | null {
  if (accept.length === 0) return null;
  const types = TYPES.filter((t) => accepts(accept, t));
  // Nothing the library stores: a filter that matches nothing it holds.
  return (types.length ? types : accept.filter((a) => !a.includes("*"))).join(",") || "none/none";
}

/** A media record of the library list as an item. */
function itemOf(record: Envelope): MediaItem {
  const d = record.data;
  const mime = String(d.mime ?? "");
  const number = (v: unknown) => (v === null || v === undefined || v === "" ? undefined : Number(v));
  return {
    id: record.key,
    name: String(d.name ?? record.title ?? record.key),
    mime,
    // i64 columns are strings on the wire.
    size: number(d.size) ?? 0,
    width: number(d.width),
    height: number(d.height),
    inline: INLINE.has(mime),
  };
}

export interface PickerRules {
  /** Several items may be chosen. */
  multiple: boolean;
  /** Most items chosen at once. */
  max: number;
  /** Accepted types (`image/*`). */
  accept: string[];
  /** Largest accepted file, in bytes. */
  maxSize?: number;
  /** Items already used (shown as such, not chosen again). */
  exclude?: string[];
}

/** Whether the user may open the library or upload (else the picker has nothing to show). */
export function usePickerAccess() {
  const panel = usePanel();
  const permissions = panel.data?.permissions ?? [];
  // Referencing a file needs both, as the server checks on save (and thumbnails need `media.view`).
  return { library: permissions.includes("media.view_any") && permissions.includes("media.view"), upload: panel.data?.uploads !== undefined };
}

export function MediaPicker({ open, onClose, onPick, title, action, rules }: {
  open: boolean;
  onClose: () => void;
  onPick: (items: MediaItem[]) => void;
  title: string;
  /** The confirm button's label. */
  action: string;
  rules: PickerRules;
}) {
  return (
    <Dialog open={open} onClose={onClose} title={title} className="max-w-3xl">
      {open && <PickerBody onClose={onClose} onPick={onPick} action={action} rules={rules} />}
    </Dialog>
  );
}

function PickerBody({ onClose, onPick, action, rules }: {
  onClose: () => void;
  onPick: (items: MediaItem[]) => void;
  action: string;
  rules: PickerRules;
}) {
  const t = useT();
  const access = usePickerAccess();
  const [tab, setTab] = useState(access.library ? "library" : "upload");
  const [chosen, setChosen] = useState<MediaItem[]>([]);
  const max = Math.max(0, rules.multiple ? rules.max : 1);
  const toggle = (item: MediaItem) =>
    setChosen((all) => {
      if (all.some((c) => c.id === item.id)) return all.filter((c) => c.id !== item.id);
      if (!rules.multiple) return [item];
      return all.length >= max ? all : [...all, item];
    });
  const add = (item: MediaItem) =>
    setChosen((all) => (all.some((c) => c.id === item.id) ? all : rules.multiple ? [...all, item].slice(0, max) : [item]));
  // The queue lives as long as the dialog: switching tabs keeps it running.
  const client = useQueryClient();
  const panel = usePanel();
  // The field's limit and the server's: a file refused by either takes no room.
  const maxBytes = Math.min(rules.maxSize ?? Infinity, panel.data?.uploads?.maxBytes ?? Infinity);
  const check = (file: File): Text | null =>
    !accepts(rules.accept, file.type)
      ? later("media.notAccepted", { file: file.name })
      : file.size > maxBytes
        ? later("media.tooLarge", { file: file.name })
        : null;
  const uploads = useUploads({
    onUploaded: (item) => {
      add(item);
      void client.invalidateQueries({ queryKey: keys.records("media") });
    },
    check,
  });
  // How many more files may be uploaded: what is still free, less every file the
  // queue still holds (on its way, in but waiting for the ones before it, or a
  // failure that may be retried). One replaces the other when a single item is chosen.
  const coming = uploads.uploads.filter((u) => u.status !== "failed" || u.retryable).length;
  const room = Math.max(0, (rules.multiple ? max - chosen.length : 1) - coming);
  return (
    <div className="flex flex-col gap-4">
      <Tabs value={tab} onValueChange={setTab}>
        <TabsList aria-label={t("picker.tabs")}>
          {access.library && <TabsTrigger value="library">{t("picker.library")}</TabsTrigger>}
          {access.upload && <TabsTrigger value="upload">{t("picker.upload")}</TabsTrigger>}
        </TabsList>
        {access.library && (
          <TabsContent value="library">
            <Library rules={rules} chosen={chosen} onToggle={toggle} full={chosen.length >= max} />
          </TabsContent>
        )}
        {access.upload && (
          <TabsContent value="upload">
            <UploadTab rules={rules} uploads={uploads} room={room} check={check} />
          </TabsContent>
        )}
        {!access.library && !access.upload && <p className="pt-4 text-sm text-muted-foreground">{t("picker.nothing")}</p>}
      </Tabs>
      <div className="flex flex-wrap items-center justify-end gap-2">
        <p className="mr-auto text-xs text-muted-foreground" aria-live="polite">
          {rules.multiple ? t("picker.chosenMany", { n: chosen.length, max }) : chosen[0] ? t("picker.chosenOne", { file: chosen[0].name }) : ""}
        </p>
        <Button type="button" variant="outline" onClick={onClose}>{t("action.cancel")}</Button>
        {/* Files still uploading would be lost: confirm once they are in (or dismissed). */}
        <Button type="button" disabled={chosen.length === 0 || uploads.busy} onClick={() => onPick(chosen)}>{action}</Button>
      </div>
    </div>
  );
}

function Library({ rules, chosen, onToggle, full }: { rules: PickerRules; chosen: MediaItem[]; onToggle: (item: MediaItem) => void; full: boolean }) {
  const t = useT();
  const id = useId();
  const { apiPath } = usePanelConfig();
  const [search, setSearch] = useState("");
  const [debounced, setDebounced] = useState("");
  const [page, setPage] = useState(1);
  useEffect(() => {
    const timer = setTimeout(() => {
      setDebounced(search.trim());
      setPage(1);
    }, 250);
    return () => clearTimeout(timer);
  }, [search]);
  const query = useMemo(() => {
    const q = new URLSearchParams({ page: String(page), per_page: String(PER_PAGE), sort: "-created_at" });
    if (debounced) q.set("search", debounced);
    const mime = mimeFilter(rules.accept);
    if (mime) q.set("filter[mime][in]", mime);
    return q;
  }, [page, debounced, rules.accept]);
  const records = useRecords("media", query);
  // Rows of the previous query, still shown while the next loads: not actionable.
  const stale = records.isPlaceholderData || records.isFetching && !records.data;
  const items = (records.data?.data ?? []).map(itemOf);
  const meta = records.data?.meta;
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-2">
        <Label htmlFor={`${id}-search`}>{t("picker.search")}</Label>
        <Input id={`${id}-search`} type="search" autoFocus value={search} onChange={(e) => setSearch(e.target.value)}
          onKeyDown={(e) => {
            // The picker sits inside the record's form: Enter never submits it.
            if (e.key === "Enter") e.preventDefault();
          }} />
      </div>
      {/* The first page only: later ones keep the previous rows (marked busy) while they load. */}
      {records.isLoading && <p role="status" className="text-sm text-muted-foreground">{t("picker.loading")}</p>}
      {records.isError && <p role="alert" className="text-sm text-destructive">{t("picker.failed")}</p>}
      {records.data && items.length === 0 && <p className="text-sm text-muted-foreground">{t("picker.empty")}</p>}
      {items.length > 0 && (
        <ul aria-label={t("picker.items")} aria-busy={stale || undefined} className="grid grid-cols-2 gap-2 sm:grid-cols-4">
          {items.map((item) => {
            const selected = chosen.some((c) => c.id === item.id);
            const used = rules.exclude?.includes(item.id) ?? false;
            const tooLarge = rules.maxSize !== undefined && item.size > rules.maxSize;
            const why: Text | null = used ? later("picker.used") : tooLarge ? later("picker.tooLarge") : null;
            const off = stale || used || tooLarge || (!selected && full && rules.multiple);
            return (
              <li key={item.id}>
                <button
                  type="button"
                  aria-pressed={selected}
                  disabled={off}
                  aria-describedby={why ? `${id}-${item.id}-why` : undefined}
                  onClick={() => onToggle(item)}
                  className={cn(
                    "flex w-full flex-col gap-1 rounded-md border p-1 text-left text-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50",
                    selected ? "border-ring bg-muted ring-2 ring-ring" : "border-border hover:bg-muted",
                  )}
                >
                  {item.inline
                    ? <img src={mediaUrl(apiPath, item.id)} alt="" loading="lazy" className="aspect-square w-full rounded object-cover" />
                    : <span className="flex aspect-square w-full items-center justify-center rounded bg-muted"><FileText className="size-8" aria-hidden="true" /></span>}
                  <span className="truncate font-medium">{item.name}</span>
                  {why && <span id={`${id}-${item.id}-why`} className="text-muted-foreground">{typeof why === "function" ? why() : why}</span>}
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {meta && meta.lastPage > 1 && (
        <nav aria-label={t("picker.pages")} className="flex items-center justify-between gap-2">
          <Button type="button" size="sm" variant="outline" disabled={page <= 1 || stale} onClick={() => setPage((p) => p - 1)}>
            <ChevronLeft /> {t("picker.previous")}
          </Button>
          <span className="text-xs text-muted-foreground">{t("picker.page", { page: meta.page, pages: meta.lastPage })}</span>
          <Button type="button" size="sm" variant="outline" disabled={page >= meta.lastPage || stale} onClick={() => setPage((p) => p + 1)}>
            {t("picker.next")} <ChevronRight />
          </Button>
        </nav>
      )}
    </div>
  );
}

function UploadTab({ rules, uploads, room, check }: {
  rules: PickerRules;
  uploads: ReturnType<typeof useUploads>;
  /** How many more files may be chosen. */
  room: number;
  check: (file: File) => Text | null;
}) {
  const t = useT();
  const id = useId();
  const input = useRef<HTMLInputElement>(null);
  const [over, setOver] = useState(false);
  // Files past what can still be chosen are not sent: how many could be, when some were left out.
  const [refused, setRefused] = useState<number | null>(null);
  const take = (files: File[]) => {
    let left = room;
    // Files refused anyway (type, size) are listed with their message; they take no place.
    const taken = files.filter((file) => check(file) !== null || left-- > 0);
    setRefused(taken.length < files.length ? room : null);
    uploads.add(taken);
  };
  return (
    <div className="flex flex-col gap-3">
      <div
        onDragOver={(e) => {
          e.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          take([...e.dataTransfer.files]);
        }}
        className={cn("flex flex-col items-center gap-2 rounded-md border-2 border-dashed p-6 text-center text-sm", over ? "border-ring bg-muted" : "border-border")}
      >
        <UploadIcon className="size-6 text-muted-foreground" aria-hidden="true" />
        <p id={`${id}-hint`} className="text-muted-foreground">{rules.multiple ? t("picker.dropMany") : t("picker.dropOne")}</p>
        <Button type="button" size="sm" variant="outline" aria-describedby={`${id}-hint`} onClick={() => input.current?.click()}>
          {t("picker.choose")}
        </Button>
        <input ref={input} type="file" hidden multiple={rules.multiple} accept={rules.accept.join(",") || undefined} aria-label={t("picker.choose")} onChange={(e) => {
          take([...(e.target.files ?? [])]);
          e.target.value = "";
        }} />
      </div>
      <p role="status" className="text-xs text-destructive">
        {refused === null ? "" : refused === 0 ? t("picker.full") : t("picker.tooMany", { count: refused })}
      </p>
      <UploadList uploads={uploads.uploads} onRetry={uploads.retry} onDismiss={uploads.dismiss} />
    </div>
  );
}
