// Relations on a record's page: the related records (listed by the server
// within their own resource's scope and policy). Has-many: a way to add one;
// belongs-to-many: attach, detach and the pivot columns of each link.
import { Link } from "@tanstack/react-router";
import { Link2, Pencil, Plus, Unlink } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useLinkOptions, useLinkWrite, useRelationRecords, useSchema } from "@/api/queries";
import type { ComponentSchema, LinkOutcome, RelationSchema } from "@/api/types";
import { SchemaFormDialog } from "@/components/schema-form-dialog";
import { ErrorState, Loading } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Cell, columnClass } from "@/renderers/table";
import { cn } from "@/lib/utils";
import { isInput } from "@/schema/form";
import { useT } from "@/i18n";

/** The components without the input of `name` (set by the server through a relation). */
export function withoutInput(components: ComponentSchema[], name: string): ComponentSchema[] {
  return components
    .filter((c) => !(isInput(c) && c.name === name))
    .map((c) => (isInput(c) ? c : { ...c, components: withoutInput(c.components, name) }));
}

type SectionProps = { parent: string; record: string; relation: RelationSchema };

export function RelationSection(props: SectionProps) {
  return props.relation.kind === "belongsToMany" ? <ManySection {...props} /> : <HasManySection {...props} />;
}

function HasManySection({ parent, record, relation }: SectionProps) {
  const t = useT();
  const schema = useSchema(relation.resource, "view");
  const [page, setPage] = useState(1);
  const [search, setSearch] = useState("");
  const via = { parent, record, relation: relation.name };
  const query = useMemo(() => {
    const params = new URLSearchParams({ page: String(page), per_page: "10" });
    if (search) params.set("search", search);
    return params;
  }, [page, search]);
  const records = useRelationRecords(relation.resource, via, query);
  // The parent itself is not repeated in every row.
  const columns = (schema.data?.table.columns ?? []).filter((c) => c.name !== relation.field);
  const meta = records.data?.meta;
  const titleId = `relation-${relation.name}`;
  return (
    <Card>
      <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-3">
        <CardTitle id={titleId}>{relation.label}</CardTitle>
        <div className="flex flex-wrap items-center gap-2">
          {schema.data?.table.searchable && (
            <Input aria-label={t("relation.search", { relation: relation.label })} className="w-48" value={search} onChange={(e) => {
              setSearch(e.target.value);
              setPage(1);
            }} />
          )}
          {relation.create && (
            <Link
              to="/r/$slug/create"
              params={{ slug: relation.resource }}
              search={{ via: parent, record, relation: relation.name }}
              className={buttonVariants({ size: "sm" })}
            >
              <Plus /> {t("relation.new")}
            </Link>
          )}
        </div>
      </CardHeader>
      <CardContent className="p-0">
        {schema.isLoading || records.isLoading ? (
          <Loading />
        ) : schema.error || records.error ? (
          <ErrorState error={schema.error ?? records.error} />
        ) : (
          <div className="max-w-full overflow-x-auto">
            <table className="w-full text-sm" aria-labelledby={titleId}>
              <thead className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
                <tr>
                  {columns.map((c) => <th key={c.name} scope="col" className={cn("p-3 font-medium", columnClass(c))}>{c.label}</th>)}
                  <th className="p-3"><span className="sr-only">{t("relation.open")}</span></th>
                </tr>
              </thead>
              <tbody>
                {(records.data?.data ?? []).length === 0 && (
                  <tr><td colSpan={columns.length + 1} className="p-6 text-center text-muted-foreground">{t("relation.empty")}</td></tr>
                )}
                {(records.data?.data ?? []).map((row) => (
                  <tr key={row.key} className="border-b border-border last:border-0">
                    {columns.map((c) => <td key={c.name} className={cn("p-3", columnClass(c))}><Cell column={c} record={row} /></td>)}
                    <td className="p-3 text-right">
                      {row.abilities.view && (
                        <Link to="/r/$slug/records/$key" params={{ slug: relation.resource, key: row.key }} className="underline">
                          {t("action.view")}
                        </Link>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {meta && meta.lastPage > 1 && (
          <div className="flex items-center justify-between border-t border-border p-3 text-sm">
            <span className="text-muted-foreground">{t("relation.page", { page: meta.page, last: meta.lastPage })}</span>
            <div className="flex gap-2">
              <button type="button" className={buttonVariants({ size: "sm", variant: "outline" })} disabled={page <= 1} onClick={() => setPage(page - 1)}>{t("list.previous")}</button>
              <button type="button" className={buttonVariants({ size: "sm", variant: "outline" })} disabled={page >= meta.lastPage} onClick={() => setPage(page + 1)}>{t("list.next")}</button>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

/** A pivot value as text (`—` when empty). */
function pivotText(value: unknown): string {
  if (value === null || value === undefined || value === "") return "—";
  return typeof value === "string" ? value : JSON.stringify(value);
}

/** Belongs-to-many: the linked records, with attach, detach and link editing as allowed. */
function ManySection({ parent, record, relation }: SectionProps) {
  const t = useT();
  const schema = useSchema(relation.resource, "view");
  const [page, setPage] = useState(1);
  const [search, setSearch] = useState("");
  const via = { parent, record, relation: relation.name };
  const query = useMemo(() => {
    const params = new URLSearchParams({ page: String(page), per_page: "10" });
    if (search) params.set("search", search);
    return params;
  }, [page, search]);
  const records = useRelationRecords(relation.resource, via, query);
  const write = useLinkWrite(relation.resource, via);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [attaching, setAttaching] = useState(false);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [optionSearch, setOptionSearch] = useState("");
  const options = useLinkOptions(via, optionSearch, attaching);
  const [detaching, setDetaching] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: string } | null>(null);
  const columns = schema.data?.table.columns ?? [];
  const pivot = relation.pivot ?? [];
  const rows = records.data?.data ?? [];
  // Rows of an earlier query still shown while the next one loads: no writes on them.
  const settled = !records.isPlaceholderData && !records.isFetching;
  // A selection belongs to the rows shown: another search, page or refresh starts over.
  const shownKeys = rows.map((r) => r.key).join(",");
  useEffect(() => {
    setSelected(new Set());
    setDetaching(false);
  }, [shownKeys, page, search]);
  const meta = records.data?.meta;
  const titleId = `relation-${relation.name}`;

  const report = (outcome: LinkOutcome, done: "relation.linked" | "relation.unlinked" | "relation.updated") => {
    const parts = [t(done, { count: outcome.processed.length })];
    if (outcome.missing.length) parts.push(t("relation.missing", { count: outcome.missing.length }));
    setNotice({ variant: outcome.missing.length ? "destructive" : "default", text: parts.join(" ") });
  };
  const toggle = (set: Set<string>, key: string, on: boolean) => {
    const next = new Set(set);
    if (on) next.add(key);
    else next.delete(key);
    return next;
  };
  const detach = async () => {
    setDetaching(false);
    // Only links still shown.
    const keys = rows.map((r) => r.key).filter((k) => selected.has(k));
    if (!settled || keys.length === 0) return;
    try {
      report(await write.mutateAsync({ op: "detach", keys }), "relation.unlinked");
      setSelected(new Set());
    } catch (e) {
      setNotice({ variant: "destructive", text: e instanceof Error ? e.message : t("relation.failed") });
    }
  };

  return (
    <Card>
      <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-3">
        <CardTitle id={titleId}>{relation.label}</CardTitle>
        <div className="flex flex-wrap items-center gap-2">
          {schema.data?.table.searchable && (
            <Input aria-label={t("relation.search", { relation: relation.label })} className="w-48" value={search} onChange={(e) => {
              setSearch(e.target.value);
              setPage(1);
            }} />
          )}
          {relation.detach && selected.size > 0 && (
            <Button size="sm" variant="outline" disabled={write.isPending || !settled} onClick={() => setDetaching(true)}>
              <Unlink /> {t("relation.detachSelected")}
            </Button>
          )}
          {relation.attach && (
            <Button size="sm" disabled={write.isPending} onClick={() => {
              setChosen(new Set());
              setOptionSearch("");
              setAttaching(true);
            }}>
              <Link2 /> {t("relation.attach")}
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="p-0">
        {notice && <Alert variant={notice.variant} className="m-3">{notice.text}</Alert>}
        {schema.isLoading || records.isLoading ? (
          <Loading />
        ) : schema.error || records.error ? (
          <ErrorState error={schema.error ?? records.error} />
        ) : (
          <div className="max-w-full overflow-x-auto">
            <table className="w-full text-sm" aria-labelledby={titleId}>
              <thead className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
                <tr>
                  {relation.detach && (
                    <th className="w-10 p-3">
                      <Checkbox
                        aria-label={t("relation.selectAll")}
                        checked={rows.length > 0 && rows.every((r) => selected.has(r.key))}
                        onChange={(e) => setSelected(e.target.checked ? new Set(rows.map((r) => r.key)) : new Set())}
                      />
                    </th>
                  )}
                  {columns.map((c) => <th key={c.name} scope="col" className={cn("p-3 font-medium", columnClass(c))}>{c.label}</th>)}
                  {pivot.map((p) => <th key={`pivot-${p.name}`} scope="col" className="p-3 font-medium">{p.label}</th>)}
                  <th className="p-3"><span className="sr-only">{t("relation.open")}</span></th>
                </tr>
              </thead>
              <tbody>
                {rows.length === 0 && (
                  <tr><td colSpan={columns.length + pivot.length + 2} className="p-6 text-center text-muted-foreground">{t("relation.empty")}</td></tr>
                )}
                {rows.map((row) => (
                  <tr key={row.key} className="border-b border-border last:border-0">
                    {relation.detach && (
                      <td className="p-3">
                        <Checkbox
                          aria-label={t("relation.selectRow", { record: row.title ?? row.key })}
                          checked={selected.has(row.key)}
                          onChange={(e) => setSelected(toggle(selected, row.key, e.target.checked))}
                        />
                      </td>
                    )}
                    {columns.map((c) => <td key={c.name} className={cn("p-3", columnClass(c))}><Cell column={c} record={row} /></td>)}
                    {pivot.map((p) => <td key={`pivot-${p.name}`} className="p-3">{pivotText(records.data?.pivot?.[row.key]?.[p.name])}</td>)}
                    <td className="p-3 text-right whitespace-nowrap">
                      {relation.pivotForm && (
                        <Button size="sm" variant="ghost" aria-label={`${t("relation.editLink")}: ${row.title ?? row.key}`} onClick={() => setEditing(row.key)}>
                          <Pencil />
                        </Button>
                      )}
                      {row.abilities.view && (
                        <Link to="/r/$slug/records/$key" params={{ slug: relation.resource, key: row.key }} className="underline">
                          {t("action.view")}
                        </Link>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {meta && meta.lastPage > 1 && (
          <div className="flex items-center justify-between border-t border-border p-3 text-sm">
            <span className="text-muted-foreground">{t("relation.page", { page: meta.page, last: meta.lastPage })}</span>
            <div className="flex gap-2">
              <button type="button" className={buttonVariants({ size: "sm", variant: "outline" })} disabled={page <= 1} onClick={() => setPage(page - 1)}>{t("list.previous")}</button>
              <button type="button" className={buttonVariants({ size: "sm", variant: "outline" })} disabled={page >= meta.lastPage} onClick={() => setPage(page + 1)}>{t("list.next")}</button>
            </div>
          </div>
        )}
      </CardContent>

      <SchemaFormDialog
        open={attaching}
        onClose={() => setAttaching(false)}
        title={t("relation.attachTitle", { relation: relation.label })}
        form={relation.attachForm}
        slug={relation.resource}
        submitLabel={t("relation.attach")}
        mode="create"
        ready={chosen.size > 0}
        onSubmit={async (data) => report(await write.mutateAsync({ op: "attach", keys: [...chosen], pivot: data }), "relation.linked")}
      >
        <fieldset className="flex flex-col gap-2">
          <legend className="mb-1 text-sm font-medium">{t("relation.choose")}</legend>
          <Input aria-label={t("relation.search", { relation: relation.label })} value={optionSearch} onChange={(e) => setOptionSearch(e.target.value)} />
          <div className="max-h-64 overflow-y-auto rounded-md border border-border">
            {options.isLoading ? (
              <Loading />
            ) : options.error ? (
              <p className="p-3 text-sm text-muted-foreground">{t("relation.loadFailed")}</p>
            ) : (options.data ?? []).length === 0 ? (
              <p className="p-3 text-sm text-muted-foreground">{t("relation.noOptions")}</p>
            ) : (
              (options.data ?? []).map((option) => (
                <label key={option.value} className="flex items-center gap-2 border-b border-border p-2 text-sm last:border-0">
                  <Checkbox
                    checked={option.attached || chosen.has(option.value)}
                    disabled={option.attached}
                    onChange={(e) => setChosen(toggle(chosen, option.value, e.target.checked))}
                  />
                  <span className="min-w-0 flex-1 truncate">{option.label}</span>
                  {option.attached && <Badge color="gray">{t("relation.attached")}</Badge>}
                </label>
              ))
            )}
          </div>
          {chosen.size === 0 && <p className="text-xs text-muted-foreground">{t("relation.chooseOne")}</p>}
        </fieldset>
      </SchemaFormDialog>

      <SchemaFormDialog
        open={editing !== null}
        onClose={() => setEditing(null)}
        title={t("relation.editLink")}
        form={relation.pivotForm}
        slug={relation.resource}
        data={editing ? records.data?.pivot?.[editing] : undefined}
        submitLabel={t("record.saveChanges")}
        mode="edit"
        onSubmit={async (data) => {
          if (editing) report(await write.mutateAsync({ op: "update", key: editing, pivot: data }), "relation.updated");
        }}
      />

      <Dialog open={detaching} onClose={() => setDetaching(false)} title={t("relation.detach")}>
        <p className="text-sm text-muted-foreground">{t("relation.confirmDetach", { count: selected.size, relation: relation.label })}</p>
        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => setDetaching(false)}>{t("action.cancel")}</Button>
          <Button variant="destructive" onClick={() => void detach()}>{t("relation.detach")}</Button>
        </div>
      </Dialog>
    </Card>
  );
}
