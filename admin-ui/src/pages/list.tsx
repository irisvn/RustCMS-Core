import { Link, useNavigate } from "@tanstack/react-router";
import { ArrowDown, ArrowUp, Eye, ListFilter, Pencil, Plus, RotateCcw, Search, Trash2, Upload, X } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useUncertain } from "@/api/context";
import { useDeleteRecord, useForceDeleteRecord, useRecords, useRestoreRecord, useRunAction, useSchema } from "@/api/queries";
import type { ActionOutcome, ActionSchema, Envelope } from "@/api/types";
import { ErrorState, Loading, PausedBanner, VersionBanner, compatibility } from "@/components/states";
import { Page, PageHeader } from "@/components/layout";
import { Button, buttonVariants } from "@/components/ui/button";
import { Table, TableBody, TableHeader, TableRow } from "@/components/ui/misc";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { type ActionNotice, ActionNoticeView, RecordActions, isOutcome, outcomeNotice, recoveredAction } from "@/components/actions";
import { SchemaFormDialog } from "@/components/schema-form-dialog";
import { ExportButton, ExportsPanel } from "@/components/exports";
import { Input, NativeSelect } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Cell, FilterControl, KNOWN_FILTERS, columnClass, type FilterParams } from "@/renderers/table";
import { cn } from "@/lib/utils";
import { MEDIA_SLUG, MediaGrid, MediaUploader, MediaViewToggle, useMediaView } from "@/renderers/media-library";
import { errorText, formatCount, later, t as translate, useT } from "@/i18n";

export type ListSearch = Record<string, string>;

/** The list's query string: only the parameters the API understands, in a stable order. */
export function listQuery(search: ListSearch): URLSearchParams {
  const params = new URLSearchParams();
  for (const key of Object.keys(search).sort()) {
    const value = search[key];
    if (!value) continue;
    if (["page", "per_page", "sort", "search", "trashed"].includes(key) || /^filter\[[^\]]+\]\[[a-z]+\]$/.test(key)) {
      params.set(key, value);
    }
  }
  return params;
}

function useDebounced(value: string, ms: number) {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return debounced;
}

const recordCount = (n: number) => translate("list.records", { count: n, n: formatCount(n) });

export { outcomeMessage } from "@/components/actions";

export function ListPage({ slug, search }: { slug: string; search: ListSearch }) {
  const t = useT();
  const navigate = useNavigate();
  const schema = useSchema(slug, "view");
  const query = useMemo(() => listQuery(search), [search]);
  // What an export takes: the list's query without its page.
  const exportQuery = useMemo(() => {
    const params = new URLSearchParams(query);
    params.delete("page");
    params.delete("per_page");
    return params;
  }, [query]);
  const records = useRecords(slug, query);
  // The media library: thumbnails in a grid, or the table.
  const media = slug === MEDIA_SLUG;
  const [mediaView, setMediaView] = useMediaView();
  const grid = media && mediaView === "grid";
  const del = useDeleteRecord(slug);
  const restore = useRestoreRecord(slug);
  const forceDelete = useForceDeleteRecord(slug);
  const run = useRunAction(slug);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<
    | { kind: "delete"; record: Envelope }
    | { kind: "force"; record: Envelope }
    | { kind: "action"; action: ActionSchema; keys: string[] }
    | null
  >(null);
  // An action with a form, being filled in for these rows.
  const [formAction, setFormAction] = useState<{ action: ActionSchema; keys: string[] } | null>(null);
  const [notice, setNotice] = useState<ActionNotice | null>(null);
  // A write with an unknown outcome (timeout, cut connection) must not be repeated blindly.
  const uncertain = useUncertain(slug);
  const [filtersOpen, setFiltersOpen] = useState(true);
  const filtersId = useId();
  const [searchText, setSearchText] = useState(search.search ?? "");
  const debouncedSearch = useDebounced(searchText, 300);
  // The URL can change without the box (back/forward, a link): the box follows,
  // unless the change is the one this box made (text typed since then is kept).
  const urlSearch = search.search ?? "";
  const [seenSearch, setSeenSearch] = useState(urlSearch);
  const [pushedSearch, setPushedSearch] = useState<string | null>(null);
  if (urlSearch !== seenSearch) {
    setSeenSearch(urlSearch);
    if (urlSearch !== pushedSearch) setSearchText(urlSearch);
  }

  const setSearch = (changes: Record<string, string | undefined>, resetPage = true) => {
    const next: ListSearch = { ...search };
    for (const [k, v] of Object.entries(changes)) {
      if (v) next[k] = v;
      else delete next[k];
    }
    if (resetPage) delete next.page;
    void navigate({ to: "/r/$slug", params: { slug }, search: next, replace: true });
  };

  useEffect(() => {
    if (urlSearch !== debouncedSearch) {
      setPushedSearch(debouncedSearch);
      setSearch({ search: debouncedSearch || undefined });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [debouncedSearch]);

  // A new page or query starts a new selection.
  const queryKey = query.toString();
  // Selection belongs to the rows on screen: cleared on a new query and when its results arrive.
  useEffect(() => setSelected(new Set()), [slug, queryKey, records.dataUpdatedAt]);
  // A confirmation belongs to the rows it was opened on.
  useEffect(() => setConfirm(null), [slug, queryKey, records.dataUpdatedAt]);
  // Rows of the previous query are kept on screen while the next loads: not actionable.
  const stale = records.isPlaceholderData;

  if (schema.isLoading) return <Loading />;
  if (schema.error || !schema.data) return <ErrorState error={schema.error} />;
  const s = schema.data;
  const { readOnly } = compatibility(s);
  const table = s.table;
  const locked = readOnly || uncertain;
  // The trash is offered when the resource has one and the user may restore or purge.
  const trash = table.softDeletes === true && (s.abilities.restore === true || s.abilities.forceDelete === true);
  const trashMode = trash && (search.trashed === "with" || search.trashed === "only") ? search.trashed : "";
  // Actions never apply to trashed records: none in the trash-only view.
  const bulkActions = locked || trashMode === "only" ? [] : table.actions.filter((a) => a.bulk);
  // Every action is a row action (`bulk` ones are also offered for selections);
  // each row's abilities say where the server offers it.
  const rowActions = locked ? [] : table.actions;
  const sort = search.sort ?? table.defaultSort ?? "";
  const rows = records.data?.data ?? [];
  const meta = records.data?.meta;

  const toggleSort = (name: string) => setSearch({ sort: sort === name ? `-${name}` : name });
  const filters = table.filters.filter((f) => KNOWN_FILTERS.has(f.type));
  const activeFilters = filters.filter((filter) => Object.entries(search).some(([key, value]) => key.startsWith(`filter[${filter.name}][`) && value));
  const clearFilters = () => setSearch(Object.fromEntries(Object.keys(search).filter((key) => /^filter\[/.test(key)).map((key) => [key, undefined])));

  // A completion opens its page only while this list is shown.
  const outcomeNoticeOf = (label: string, outcome: ActionOutcome) => (mounted.current ? outcomeNotice(label, outcome) : null);
  /** "Retry safely" of an action: its outcome, as if it had answered the first time. */
  const recovered = (result: unknown, pending: { path: string }) => {
    const name = recoveredAction(pending.path);
    if (!name || !isOutcome(result)) return false;
    setNotice(outcomeNoticeOf(table.actions.find((a) => a.name === name)?.label ?? name, result));
    // The banner still says the change was applied.
    return false;
  };
  const runAction = async (action: ActionSchema, requested: string[]) => {
    setNotice(null);
    // Only keys of the records currently displayed.
    const shown = new Set(rows.map((r) => r.key));
    const keys = requested.filter((k) => shown.has(k));
    if (stale || keys.length === 0) return;
    try {
      const outcome = await run.mutateAsync({ action: action.name, keys });
      setNotice(outcomeNoticeOf(action.label, outcome));
      setSelected(new Set());
    } catch (e) {
      setNotice({ variant: "destructive", text: errorText(e, "action.failed") });
    }
  };
  const confirmed = async () => {
    if (!confirm) return;
    const current = confirm;
    setConfirm(null);
    if (current.kind !== "action") {
      // Still shown, current and allowed (rows may have been refreshed meanwhile).
      const row = rows.find((r) => r.key === current.record.key);
      const ability = current.kind === "delete" ? "delete" : "force_delete";
      if (stale || locked || !row || !row.abilities[ability]) {
        setNotice({ variant: "destructive", text: later("list.changedMeanwhile") });
        return;
      }
    }
    if (current.kind === "delete") {
      try {
        await del.mutateAsync(current.record.key);
        setNotice({ variant: "default", text: later(table.softDeletes ? "list.trashed" : "list.deleted") });
      } catch (e) {
          setNotice({ variant: "destructive", text: errorText(e, "list.deleteFailed") });
      }
    } else if (current.kind === "force") {
      try {
        await forceDelete.mutateAsync(current.record.key);
        setNotice({ variant: "default", text: later("list.deletedForGood") });
      } catch (e) {
        setNotice({ variant: "destructive", text: errorText(e, "list.deleteFailed") });
      }
    } else {
      await runAction(current.action, current.keys);
    }
  };
  const restoreRecord = async (record: Envelope) => {
    setNotice(null);
    try {
      await restore.mutateAsync(record.key);
      setNotice({ variant: "default", text: later("list.restored") });
    } catch (e) {
      setNotice({ variant: "destructive", text: errorText(e, "list.restoreFailed") });
    }
  };
  const startAction = (action: ActionSchema, keys: string[]) =>
    action.form
      ? setFormAction({ action, keys })
      : action.confirm
        ? setConfirm({ kind: "action", action, keys })
        : void runAction(action, keys);
  /** Runs an action with its form's values; errors go back to the dialog. */
  const submitForm = async (data: Record<string, unknown>) => {
    if (!formAction) return;
    setNotice(null);
    const shown = new Set(rows.map((r) => r.key));
    const keys = formAction.keys.filter((k) => shown.has(k));
    if (stale || keys.length === 0) throw new Error(translate("list.changedMeanwhile"));
    const outcome = await run.mutateAsync({ action: formAction.action.name, keys, data });
    setNotice(outcomeNoticeOf(formAction.action.label, outcome));
    setSelected(new Set());
  };

  /** A record's actions (table rows and grid cards). */
  const rowActionsOf = (record: Envelope) => (
    <div className="flex justify-end gap-1">
      <RecordActions actions={rowActions} record={record} disabled={run.isPending || stale} onStart={(a) => startAction(a, [record.key])} />
      {record.abilities.view && !record.trashed && (
        <Link
          to="/r/$slug/records/$key"
          params={{ slug, key: record.key }}
          aria-label={t("action.view")}
          className="inline-flex size-9 items-center justify-center rounded-md hover:bg-muted [&_svg]:size-4"
        >
          <Eye />
        </Link>
      )}
      {record.abilities.update && !readOnly && (
        <Link
          to="/r/$slug/records/$key/edit"
          params={{ slug, key: record.key }}
          aria-label={t("action.edit")}
          className="inline-flex size-9 items-center justify-center rounded-md hover:bg-muted [&_svg]:size-4"
        >
          <Pencil />
        </Link>
      )}
      {record.abilities.delete && !locked && (
        <Button size="icon" variant="ghost" aria-label={t("action.delete")} disabled={del.isPending || stale} onClick={() => setConfirm({ kind: "delete", record })}>
          <Trash2 />
        </Button>
      )}
      {record.trashed && record.abilities.restore && !locked && (
        <Button size="sm" variant="ghost" disabled={restore.isPending || stale} onClick={() => void restoreRecord(record)}>
          <RotateCcw /> {t("action.restore")}
        </Button>
      )}
      {record.trashed && record.abilities.force_delete && !locked && (
        <Button size="sm" variant="ghost" className="text-destructive" disabled={forceDelete.isPending || stale} onClick={() => setConfirm({ kind: "force", record })}>
          {t("action.forceDelete")}
        </Button>
      )}
    </div>
  );
  const selectOf = (record: Envelope) => (
    <Checkbox
      aria-label={t("list.select", { record: record.title ?? record.key })}
      disabled={stale}
      checked={selected.has(record.key)}
      onChange={(e) => {
        const next = new Set(selected);
        if (e.target.checked) next.add(record.key);
        else next.delete(record.key);
        setSelected(next);
      }}
    />
  );

  return (
    <Page>
      <PageHeader title={s.resource.pluralLabel} description={t("list.description", { resource: s.resource.pluralLabel.toLocaleLowerCase() })} actions={
        <div className="flex flex-wrap items-start gap-2">
          {media && <MediaViewToggle view={mediaView} onChange={setMediaView} />}
          {media && s.abilities.upload && !readOnly && <MediaUploader />}
          {s.abilities.export && <ExportButton slug={slug} query={exportQuery} />}
          {s.abilities.import && !readOnly && (
            <Link to="/r/$slug/import" params={{ slug }} className={buttonVariants({ variant: "outline" })}>
              <Upload /> {t("import.button")}
            </Link>
          )}
          {s.abilities.create && !readOnly && !uncertain && (
            <Button onClick={() => navigate({ to: "/r/$slug/create", params: { slug } })}>
              <Plus /> {t("list.new", { resource: s.resource.label.toLocaleLowerCase() })}
            </Button>
          )}
        </div>
      } />
      <VersionBanner schema={s} />
      {notice && <ActionNoticeView notice={notice} />}
      <PausedBanner scope={slug} onRecovered={recovered} />

      <Card>
        <div className="space-y-4 border-b border-border p-4 sm:p-5">
          <div className="flex flex-wrap items-center gap-3">
            {table.searchable && (
              <div className="relative w-full sm:max-w-xs">
                <Search className="pointer-events-none absolute left-3 top-3 size-4 text-muted-foreground" aria-hidden="true" />
                <Input aria-label={t("list.search")} placeholder={t("list.searchPlaceholder", { resource: s.resource.pluralLabel.toLocaleLowerCase() })} className="pl-9" value={searchText} onChange={(e) => setSearchText(e.target.value)} />
              </div>
            )}
            {filters.length > 0 && <Button variant="outline" aria-expanded={filtersOpen} aria-controls={filtersId} onClick={() => setFiltersOpen((value) => !value)}><ListFilter />{t("list.filters")}{activeFilters.length > 0 && <span className="rounded bg-muted px-1.5 text-xs">{activeFilters.length}</span>}</Button>}
            {activeFilters.length > 0 && <Button variant="ghost" size="sm" onClick={clearFilters}><X />{t("list.clearFilters")}</Button>}
            {trash && (
              <NativeSelect aria-label={t("list.trashedRecords")} className="w-44" value={trashMode} onChange={(e) => setSearch({ trashed: e.target.value || undefined })}>
                <option value="">{t("list.withoutTrashed")}</option>
                <option value="with">{t("list.withTrashed")}</option>
                <option value="only">{t("list.onlyTrashed")}</option>
              </NativeSelect>
            )}
            {meta && <span className="ml-auto text-xs text-muted-foreground">{recordCount(meta.total)}</span>}
          </div>
          {filters.length > 0 && (
            <div id={filtersId} hidden={!filtersOpen}>
              <div className="flex flex-wrap items-end gap-4 rounded-lg bg-muted/50 p-3">
                {filters.map((f) => <FilterControl key={f.name} slug={slug} filter={f} params={search as FilterParams} onChange={(changes) => setSearch(changes)} />)}
              </div>
            </div>
          )}
          {!filtersOpen && activeFilters.length > 0 && <p className="text-xs text-muted-foreground">{t("list.filteringBy", { filters: activeFilters.map((f) => f.label).join(t("list.separator")) })}</p>}
        </div>
        {bulkActions.length > 0 && selected.size > 0 && !stale && (
          <div className="flex flex-wrap items-center gap-2 border-b border-border bg-muted/50 px-4 py-3 text-sm sm:px-5">
            <span className="mr-2 font-medium">{t("list.selected", { count: selected.size, n: formatCount(selected.size) })}</span>
            {bulkActions.map((a) => <Button key={a.name} size="sm" variant={a.color === "danger" ? "destructive" : "outline"} disabled={run.isPending} onClick={() => startAction(a, [...selected])}>{a.label}</Button>)}
          </div>
        )}

        {records.error ? (
          <ErrorState error={records.error} />
        ) : grid ? (
          <>
            {bulkActions.length > 0 && rows.length > 0 && (
              <label className="flex items-center gap-2 px-4 pt-4 text-sm sm:px-5">
                <Checkbox
                  disabled={stale}
                  checked={rows.every((r) => selected.has(r.key))}
                  indeterminate={rows.some((r) => selected.has(r.key)) && !rows.every((r) => selected.has(r.key))}
                  onChange={(e) => setSelected(e.target.checked ? new Set(rows.map((r) => r.key)) : new Set())}
                />
                {t("list.selectAll")}
              </label>
            )}
            {records.isLoading && <Loading />}
            {!records.isLoading && rows.length === 0 && <p className="p-8 text-center text-muted-foreground">{t("list.empty")}</p>}
            <MediaGrid
              rows={rows}
              select={bulkActions.length > 0 ? selectOf : undefined}
              actions={rowActionsOf}
              badge={(record) => record.trashed && <Badge className="self-start">{t("list.trashedBadge")}</Badge>}
            />
          </>
        ) : (
          <Table>
              <TableHeader>
                <tr>
                  {bulkActions.length > 0 && (
                    <th className="w-8 p-3">
                      <Checkbox
                        aria-label={t("list.selectAll")}
                        disabled={stale}
                        checked={rows.length > 0 && rows.every((r) => selected.has(r.key))}
                        indeterminate={rows.some((r) => selected.has(r.key)) && !rows.every((r) => selected.has(r.key))}
                        onChange={(e) => setSelected(e.target.checked ? new Set(rows.map((r) => r.key)) : new Set())}
                      />
                    </th>
                  )}
                  {table.columns.map((c) => (
                    <th key={c.name} scope="col" className={cn("whitespace-nowrap p-3 font-medium", columnClass(c))} aria-sort={c.sortable ? sort === c.name ? "ascending" : sort === `-${c.name}` ? "descending" : "none" : undefined}>
                      {c.sortable ? (
                        <button type="button" className="inline-flex min-h-8 items-center gap-1 rounded-sm hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" onClick={() => toggleSort(c.name)}>
                          {c.label}
                          {sort === c.name && <ArrowUp className="size-3" aria-label={t("list.ascending")} />}
                          {sort === `-${c.name}` && <ArrowDown className="size-3" aria-label={t("list.descending")} />}
                        </button>
                      ) : (
                        c.label
                      )}
                    </th>
                  ))}
                  <th className="p-3 text-right">
                    <span className="sr-only">{t("list.actions")}</span>
                  </th>
                </tr>
              </TableHeader>
              <TableBody>
                {records.isLoading && (
                  <tr>
                    <td colSpan={table.columns.length + 2}>
                      <Loading />
                    </td>
                  </tr>
                )}
                {!records.isLoading && rows.length === 0 && (
                  <tr>
                    <td colSpan={table.columns.length + 2} className="p-8 text-center text-muted-foreground">
                      {t("list.empty")}
                    </td>
                  </tr>
                )}
                {rows.map((record) => (
                  <TableRow key={record.key} data-selected={selected.has(record.key) || undefined} className="data-[selected=true]:bg-muted/60">
                    {bulkActions.length > 0 && (
                      <td className="p-3">{selectOf(record)}</td>
                    )}
                    {table.columns.map((c, i) => (
                      <td key={c.name} className={cn("p-3", columnClass(c))}>
                        <Cell column={c} record={record} />
                        {i === 0 && record.trashed && <Badge className="ml-2">{t("list.trashedBadge")}</Badge>}
                      </td>
                    ))}
                    <td className="p-3">
                      {rowActionsOf(record)}
                    </td>
                  </TableRow>
                ))}
              </TableBody>
          </Table>
        )}

        {meta && (
          <div className="flex flex-wrap items-center justify-between gap-2 border-t border-border p-3 text-sm">
            <span className="text-muted-foreground">
              {meta.total === 0 ? t("list.noResults") : t("list.page", { page: formatCount(meta.page), last: formatCount(meta.lastPage), records: recordCount(meta.total) })}
            </span>
            <div className="flex items-center gap-2">
              <NativeSelect
                aria-label={t("list.perPage")}
                className="w-20"
                value={String(meta.perPage)}
                onChange={(e) => setSearch({ per_page: e.target.value })}
              >
                {table.perPageOptions.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </NativeSelect>
              <Button size="sm" variant="outline" disabled={meta.page <= 1} onClick={() => setSearch({ page: String(meta.page - 1) }, false)}>
                {t("list.previous")}
              </Button>
              <Button size="sm" variant="outline" disabled={meta.page >= meta.lastPage} onClick={() => setSearch({ page: String(meta.page + 1) }, false)}>
                {t("list.next")}
              </Button>
            </div>
          </div>
        )}
      </Card>
      {s.abilities.export && <ExportsPanel slug={slug} />}

      <SchemaFormDialog
        open={formAction !== null}
        onClose={() => setFormAction(null)}
        title={formAction?.action.label ?? ""}
        form={formAction?.action.form}
        slug={slug}
        optionsPath={(field) => `/resources/${encodeURIComponent(slug)}/actions/${encodeURIComponent(formAction?.action.name ?? "")}/fields/${encodeURIComponent(field)}/options`}
        submitLabel={translate("action.run")}
        onSubmit={submitForm}
      >
        {formAction?.action.confirm && <p className="text-sm text-muted-foreground">{formAction.action.confirm}</p>}
      </SchemaFormDialog>

      <Dialog open={confirm !== null} onClose={() => setConfirm(null)} title={confirm?.kind === "action" ? confirm.action.label : t("list.confirmDelete")}>
        <p className="text-sm text-muted-foreground">
          {confirm?.kind === "delete"
            ? table.softDeletes
              ? t("list.confirmTrash", { record: confirm.record.title ?? confirm.record.key })
              : t("list.confirmDeleteText", { record: confirm.record.title ?? confirm.record.key })
            : confirm?.kind === "force"
              ? t("list.confirmForce", { record: confirm.record.title ?? confirm.record.key })
            : confirm?.kind === "action"
              ? confirm.action.confirm
              : ""}
        </p>
        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => setConfirm(null)}>
            {t("action.cancel")}
          </Button>
          <Button variant={confirm?.kind === "delete" || confirm?.kind === "force" ? "destructive" : "default"} onClick={() => void confirmed()}>
            {t("action.confirm")}
          </Button>
        </div>
      </Dialog>
    </Page>
  );
}
