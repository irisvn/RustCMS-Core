import { Link, useNavigate } from "@tanstack/react-router";
import { ArrowDown, ArrowUp, Eye, ListFilter, Pencil, Plus, RotateCcw, Search, Trash2, X } from "lucide-react";
import { useEffect, useId, useMemo, useState } from "react";
import { useUncertain } from "@/api/context";
import { useDeleteRecord, useForceDeleteRecord, useRecords, useRestoreRecord, useRunAction, useSchema } from "@/api/queries";
import type { ActionOutcome, ActionSchema, Envelope } from "@/api/types";
import { ErrorState, Loading, PausedBanner, VersionBanner, compatibility } from "@/components/states";
import { Page, PageHeader } from "@/components/layout";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { Input, NativeSelect } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Cell, FilterControl, KNOWN_FILTERS, type FilterParams } from "@/renderers/table";

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

const count = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

export function outcomeMessage(label: string, outcome: ActionOutcome): string {
  const parts = [`${label}: ${count(outcome.processed.length, "record")} done`];
  if (outcome.denied.length) parts.push(`${count(outcome.denied.length, "record")} not allowed`);
  if (outcome.missing.length) parts.push(`${count(outcome.missing.length, "record")} no longer available`);
  return parts.join(", ") + ".";
}

export function ListPage({ slug, search }: { slug: string; search: ListSearch }) {
  const navigate = useNavigate();
  const schema = useSchema(slug, "view");
  const query = useMemo(() => listQuery(search), [search]);
  const records = useRecords(slug, query);
  const del = useDeleteRecord(slug);
  const restore = useRestoreRecord(slug);
  const forceDelete = useForceDeleteRecord(slug);
  const run = useRunAction(slug);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<
    | { kind: "delete"; record: Envelope }
    | { kind: "force"; record: Envelope }
    | { kind: "action"; action: ActionSchema; keys: string[] }
    | null
  >(null);
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: string } | null>(null);
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
  const rowActions = locked ? [] : table.actions.filter((a) => !a.bulk);
  const sort = search.sort ?? table.defaultSort ?? "";
  const rows = records.data?.data ?? [];
  const meta = records.data?.meta;

  const toggleSort = (name: string) => setSearch({ sort: sort === name ? `-${name}` : name });
  const filters = table.filters.filter((f) => KNOWN_FILTERS.has(f.type));
  const activeFilters = filters.filter((filter) => Object.entries(search).some(([key, value]) => key.startsWith(`filter[${filter.name}][`) && value));
  const clearFilters = () => setSearch(Object.fromEntries(Object.keys(search).filter((key) => /^filter\[/.test(key)).map((key) => [key, undefined])));

  const runAction = async (action: ActionSchema, requested: string[]) => {
    setNotice(null);
    // Only keys of the records currently displayed.
    const shown = new Set(rows.map((r) => r.key));
    const keys = requested.filter((k) => shown.has(k));
    if (stale || keys.length === 0) return;
    try {
      const outcome = await run.mutateAsync({ action: action.name, keys });
      setNotice({ variant: outcome.processed.length ? "default" : "destructive", text: outcomeMessage(action.label, outcome) });
      setSelected(new Set());
    } catch (e) {
      setNotice({ variant: "destructive", text: e instanceof Error ? e.message : "The action failed." });
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
        setNotice({ variant: "destructive", text: "This record changed meanwhile; nothing was done." });
        return;
      }
    }
    if (current.kind === "delete") {
      try {
        await del.mutateAsync(current.record.key);
        setNotice({ variant: "default", text: table.softDeletes ? "Moved to the trash." : "Deleted." });
      } catch (e) {
          setNotice({ variant: "destructive", text: e instanceof Error ? e.message : "Could not delete." });
      }
    } else if (current.kind === "force") {
      try {
        await forceDelete.mutateAsync(current.record.key);
        setNotice({ variant: "default", text: "Deleted permanently." });
      } catch (e) {
        setNotice({ variant: "destructive", text: e instanceof Error ? e.message : "Could not delete." });
      }
    } else {
      await runAction(current.action, current.keys);
    }
  };
  const restoreRecord = async (record: Envelope) => {
    setNotice(null);
    try {
      await restore.mutateAsync(record.key);
      setNotice({ variant: "default", text: "Restored." });
    } catch (e) {
      setNotice({ variant: "destructive", text: e instanceof Error ? e.message : "Could not restore." });
    }
  };
  const startAction = (action: ActionSchema, keys: string[]) =>
    action.confirm ? setConfirm({ kind: "action", action, keys }) : void runAction(action, keys);

  return (
    <Page>
      <PageHeader title={s.resource.pluralLabel} description={`Browse and manage your ${s.resource.pluralLabel.toLowerCase()}.`} actions={s.abilities.create && !readOnly && !uncertain && (
        <Button onClick={() => navigate({ to: "/r/$slug/create", params: { slug } })}>
          <Plus /> New {s.resource.label.toLowerCase()}
        </Button>
      )} />
      <VersionBanner schema={s} />
      {notice && <Alert variant={notice.variant}>{notice.text}</Alert>}
      <PausedBanner scope={slug} />

      <Card>
        <div className="space-y-4 border-b border-border p-4 sm:p-5">
          <div className="flex flex-wrap items-center gap-3">
            {table.searchable && (
              <div className="relative w-full sm:max-w-xs">
                <Search className="pointer-events-none absolute left-3 top-3 size-4 text-muted-foreground" aria-hidden="true" />
                <Input aria-label="Search" placeholder={`Search ${s.resource.pluralLabel.toLowerCase()}…`} className="pl-9" value={searchText} onChange={(e) => setSearchText(e.target.value)} />
              </div>
            )}
            {filters.length > 0 && <Button variant="outline" aria-expanded={filtersOpen} aria-controls={filtersId} onClick={() => setFiltersOpen((value) => !value)}><ListFilter />Filters{activeFilters.length > 0 && <span className="rounded bg-muted px-1.5 text-xs">{activeFilters.length}</span>}</Button>}
            {activeFilters.length > 0 && <Button variant="ghost" size="sm" onClick={clearFilters}><X />Clear filters</Button>}
            {trash && (
              <NativeSelect aria-label="Trashed records" className="w-44" value={trashMode} onChange={(e) => setSearch({ trashed: e.target.value || undefined })}>
                <option value="">Without trashed</option>
                <option value="with">With trashed</option>
                <option value="only">Only trashed</option>
              </NativeSelect>
            )}
            {meta && <span className="ml-auto text-xs text-muted-foreground">{count(meta.total, "record")}</span>}
          </div>
          {filters.length > 0 && (
            <div id={filtersId} hidden={!filtersOpen}>
              <div className="flex flex-wrap items-end gap-4 rounded-lg bg-muted/50 p-3">
                {filters.map((f) => <FilterControl key={f.name} slug={slug} filter={f} params={search as FilterParams} onChange={(changes) => setSearch(changes)} />)}
              </div>
            </div>
          )}
          {!filtersOpen && activeFilters.length > 0 && <p className="text-xs text-muted-foreground">Filtering by {activeFilters.map((f) => f.label).join(", ")}</p>}
        </div>
        {bulkActions.length > 0 && selected.size > 0 && !stale && (
          <div className="flex flex-wrap items-center gap-2 border-b border-border bg-muted/50 px-4 py-3 text-sm sm:px-5">
            <span className="mr-2 font-medium">{count(selected.size, "selected record")}</span>
            {bulkActions.map((a) => <Button key={a.name} size="sm" variant={a.color === "danger" ? "destructive" : "outline"} disabled={run.isPending} onClick={() => startAction(a, [...selected])}>{a.label}</Button>)}
          </div>
        )}

        {records.error ? (
          <ErrorState error={records.error} />
        ) : (
          <div className="relative max-w-full overflow-x-auto">
            <table className="w-full text-sm">
              <thead className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
                <tr>
                  {bulkActions.length > 0 && (
                    <th className="w-8 p-3">
                      <Checkbox
                        aria-label="Select all"
                        disabled={stale}
                        checked={rows.length > 0 && rows.every((r) => selected.has(r.key))}
                        indeterminate={rows.some((r) => selected.has(r.key)) && !rows.every((r) => selected.has(r.key))}
                        onChange={(e) => setSelected(e.target.checked ? new Set(rows.map((r) => r.key)) : new Set())}
                      />
                    </th>
                  )}
                  {table.columns.map((c) => (
                    <th key={c.name} scope="col" className="whitespace-nowrap p-3 font-medium" aria-sort={c.sortable ? sort === c.name ? "ascending" : sort === `-${c.name}` ? "descending" : "none" : undefined}>
                      {c.sortable ? (
                        <button type="button" className="inline-flex min-h-8 items-center gap-1 rounded-sm hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" onClick={() => toggleSort(c.name)}>
                          {c.label}
                          {sort === c.name && <ArrowUp className="size-3" aria-label="ascending" />}
                          {sort === `-${c.name}` && <ArrowDown className="size-3" aria-label="descending" />}
                        </button>
                      ) : (
                        c.label
                      )}
                    </th>
                  ))}
                  <th className="p-3 text-right">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
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
                      No records found.
                    </td>
                  </tr>
                )}
                {rows.map((record) => (
                  <tr key={record.key} data-selected={selected.has(record.key) || undefined} className="border-b border-border last:border-0 hover:bg-muted/50 data-[selected=true]:bg-muted/60">
                    {bulkActions.length > 0 && (
                      <td className="p-3">
                        <Checkbox
                          aria-label={`Select ${record.title ?? record.key}`}
                          disabled={stale}
                          checked={selected.has(record.key)}
                          onChange={(e) => {
                            const next = new Set(selected);
                            if (e.target.checked) next.add(record.key);
                            else next.delete(record.key);
                            setSelected(next);
                          }}
                        />
                      </td>
                    )}
                    {table.columns.map((c, i) => (
                      <td key={c.name} className="p-3">
                        <Cell column={c} record={record} />
                        {i === 0 && record.trashed && <Badge className="ml-2">Trashed</Badge>}
                      </td>
                    ))}
                    <td className="p-3">
                      <div className="flex justify-end gap-1">
                        {rowActions
                          .filter((a) => record.abilities[a.name])
                          .map((a) => (
                            <Button key={a.name} size="sm" variant="ghost" disabled={run.isPending || stale} onClick={() => startAction(a, [record.key])}>
                              {a.label}
                            </Button>
                          ))}
                        {record.abilities.view && !record.trashed && (
                          <Link
                            to="/r/$slug/records/$key"
                            params={{ slug, key: record.key }}
                            aria-label="View"
                            className="inline-flex size-9 items-center justify-center rounded-md hover:bg-muted [&_svg]:size-4"
                          >
                            <Eye />
                          </Link>
                        )}
                        {record.abilities.update && !readOnly && (
                          <Link
                            to="/r/$slug/records/$key/edit"
                            params={{ slug, key: record.key }}
                            aria-label="Edit"
                            className="inline-flex size-9 items-center justify-center rounded-md hover:bg-muted [&_svg]:size-4"
                          >
                            <Pencil />
                          </Link>
                        )}
                        {record.abilities.delete && !locked && (
                          <Button size="icon" variant="ghost" aria-label="Delete" disabled={del.isPending || stale} onClick={() => setConfirm({ kind: "delete", record })}>
                            <Trash2 />
                          </Button>
                        )}
                        {record.trashed && record.abilities.restore && !locked && (
                          <Button size="sm" variant="ghost" disabled={restore.isPending || stale} onClick={() => void restoreRecord(record)}>
                            <RotateCcw /> Restore
                          </Button>
                        )}
                        {record.trashed && record.abilities.force_delete && !locked && (
                          <Button size="sm" variant="ghost" className="text-destructive" disabled={forceDelete.isPending || stale} onClick={() => setConfirm({ kind: "force", record })}>
                            Delete permanently
                          </Button>
                        )}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {meta && (
          <div className="flex flex-wrap items-center justify-between gap-2 border-t border-border p-3 text-sm">
            <span className="text-muted-foreground">
              {meta.total === 0 ? "No results" : `Page ${meta.page} of ${meta.lastPage} · ${count(meta.total, "record")}`}
            </span>
            <div className="flex items-center gap-2">
              <NativeSelect
                aria-label="Per page"
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
                Previous
              </Button>
              <Button size="sm" variant="outline" disabled={meta.page >= meta.lastPage} onClick={() => setSearch({ page: String(meta.page + 1) }, false)}>
                Next
              </Button>
            </div>
          </div>
        )}
      </Card>

      <Dialog open={confirm !== null} onClose={() => setConfirm(null)} title={confirm?.kind === "action" ? confirm.action.label : "Delete record?"}>
        <p className="text-sm text-muted-foreground">
          {confirm?.kind === "delete"
            ? table.softDeletes
              ? `"${confirm.record.title ?? confirm.record.key}" will be moved to the trash.`
              : `"${confirm.record.title ?? confirm.record.key}" will be deleted. This cannot be undone.`
            : confirm?.kind === "force"
              ? `"${confirm.record.title ?? confirm.record.key}" will be deleted permanently. This cannot be undone.`
            : confirm?.kind === "action"
              ? confirm.action.confirm
              : ""}
        </p>
        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => setConfirm(null)}>
            Cancel
          </Button>
          <Button variant={confirm?.kind === "delete" || confirm?.kind === "force" ? "destructive" : "default"} onClick={() => void confirmed()}>
            Confirm
          </Button>
        </div>
      </Dialog>
    </Page>
  );
}
