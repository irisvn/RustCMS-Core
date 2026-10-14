// Has-many relations on a record's page: the related records (listed by the
// server within their own resource's scope and policy) and a way to add one.
import { Link } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { useMemo, useState } from "react";
import { useRelationRecords, useSchema } from "@/api/queries";
import type { ComponentSchema, RelationSchema } from "@/api/types";
import { ErrorState, Loading } from "@/components/states";
import { buttonVariants } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Cell } from "@/renderers/table";
import { isInput } from "@/schema/form";
import { useT } from "@/i18n";

/** The components without the input of `name` (set by the server through a relation). */
export function withoutInput(components: ComponentSchema[], name: string): ComponentSchema[] {
  return components
    .filter((c) => !(isInput(c) && c.name === name))
    .map((c) => (isInput(c) ? c : { ...c, components: withoutInput(c.components, name) }));
}

export function RelationSection({ parent, record, relation }: { parent: string; record: string; relation: RelationSchema }) {
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
                  {columns.map((c) => <th key={c.name} scope="col" className="p-3 font-medium">{c.label}</th>)}
                  <th className="p-3"><span className="sr-only">{t("relation.open")}</span></th>
                </tr>
              </thead>
              <tbody>
                {(records.data?.data ?? []).length === 0 && (
                  <tr><td colSpan={columns.length + 1} className="p-6 text-center text-muted-foreground">{t("relation.empty")}</td></tr>
                )}
                {(records.data?.data ?? []).map((row) => (
                  <tr key={row.key} className="border-b border-border last:border-0">
                    {columns.map((c) => <td key={c.name} className="p-3"><Cell column={c} record={row} /></td>)}
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
