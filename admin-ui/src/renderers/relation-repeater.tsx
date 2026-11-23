// A relationship repeater (ADR 0040): child records edited in the parent's
// form. The stored rows are read from the relation's list (the child's scope,
// policy and read rules); the form keeps only what the user changed.
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronUp, Plus, Trash2 } from "lucide-react";
import type { InputSchema, LinkPage } from "@/api/types";
import { useApi } from "@/api/context";
import { Button } from "@/components/ui/button";
import { useT } from "@/i18n";
import { cn } from "@/lib/utils";
import { type RelationRow, type RelationState, createView, newRef, parseState, relationRowProblems, stateOf } from "@/schema/relation-rows";
import { type Row, hiddenInRow, itemLabel, rowOf } from "@/schema/repeater";
import type { FormContext, WidgetProps } from "./form";
import { useOuter } from "./outer";
import { IconButton, RowComponents, TABLE_BAR, TABLE_BODY, TABLE_ROW, TableHeader, tableOf } from "./repeater";

/** How many stored rows a relationship repeater reads (more: edited in the relation section). */
const pageOf = (input: InputSchema) => {
  const limit = input.repeater?.relationship?.rowLimit ?? 50;
  return Math.min(input.repeater?.maxItems ?? limit, limit);
};

/** The stored rows of relationship repeater `input` on the edited record, through the relation's list. */
export function useStoredRows(input: InputSchema, ctx: FormContext) {
  const api = useApi();
  const relationship = input.repeater!.relationship!;
  return useQuery({
    enabled: !!ctx.record,
    // Read again each time the form opens: a save changes the rows.
    refetchOnMount: "always",
    queryKey: ["records", relationship.resource, "relation", ctx.slug, ctx.record, relationship.relation, "repeater"],
    queryFn: () =>
      api.get<LinkPage>(
        `/resources/${encodeURIComponent(ctx.slug)}/records/${encodeURIComponent(ctx.record ?? "")}/relations/${encodeURIComponent(relationship.relation)}`,
        new URLSearchParams({ per_page: String(pageOf(input)) }),
      ),
  });
}

export function RelationRepeaterWidget({ id, input, ctx, value, onChange, disabled, invalid, describedBy }: WidgetProps) {
  const t = useT();
  const outer = useOuter();
  const repeater = input.repeater!;
  const relationship = repeater.relationship!;
  const editing = !!ctx.record;
  const perPage = pageOf(input);
  const stored = useStoredRows(input, ctx);
  if (editing && stored.isLoading) return <p className="text-sm text-muted-foreground" role="status">{t("state.loading")}</p>;
  if (editing && (stored.isError || !stored.data)) return <p className="text-sm text-destructive">{t("repeater.unavailable")}</p>;
  // More rows than one page: edited in the relation section instead.
  const tooMany = editing && (stored.data?.meta.total ?? 0) > perPage;
  const state: RelationState = parseState(value) ?? (stored.data ? stateOf(repeater, stored.data.data) : { rows: [], deleted: [] });
  const locked = disabled || tooMany;

  const server = (ctx.errors[input.name] as { types?: Record<string, unknown> } | undefined)?.types ?? {};
  // The panel's problems are found by position; the server's by key or ref.
  const local = ctx.submitCount ? relationRowProblems(repeater, state.rows, outer) : {};
  const idOf = (row: RelationRow) => row.key ?? row.ref ?? "";
  const problem = (index: number) => (path: string) => {
    const id = idOf(state.rows[index]!);
    const rest = path.slice(path.indexOf(".") + 1);
    const found = server[`${id}.${rest}`] ?? local[`${index}.${rest}`];
    return typeof found === "string" ? found : Array.isArray(found) ? found.join(" ") : undefined;
  };
  const commit = (next: RelationState) => onChange(JSON.stringify(next));
  const canCreate = !locked && relationship.create && repeater.addable && (repeater.maxItems === undefined || state.rows.length < repeater.maxItems);
  const add = () => commit({ ...state, rows: [...state.rows, { ref: newRef(), values: rowOf(createView(repeater), {}), changed: [] }] });
  const remove = (index: number) => {
    const row = state.rows[index]!;
    commit({ ...state, rows: state.rows.filter((_, i) => i !== index), deleted: row.key ? [...state.deleted, row.key] : state.deleted });
  };
  const move = (from: number, to: number) => {
    if (to < 0 || to >= state.rows.length) return;
    const rows = [...state.rows];
    rows.splice(to, 0, rows.splice(from, 1)[0]!);
    commit({ ...state, rows, reordered: true });
  };
  const setRow = (index: number, values: Row) =>
    commit({
      ...state,
      rows: state.rows.map((r, i) => {
        if (i !== index) return r;
        const changed = Object.keys(values).filter((k) => values[k] !== r.values[k] || r.changed.includes(k));
        return { ...r, values, changed };
      }),
    });
  const listProblem = server[""] ?? local[""];
  const table = tableOf(repeater, (repeater.reorderable && !locked ? 2 : 0) + (!locked && repeater.deletable ? 1 : 0));

  return (
    <div id={id} className="flex flex-col gap-3" aria-describedby={describedBy} aria-invalid={invalid || undefined}>
      {tooMany && <p className="text-sm text-muted-foreground">{t("repeater.tooMany")}</p>}
      {state.rows.length === 0 && <p className="text-sm text-muted-foreground">{t("repeater.empty")}</p>}
      {table && state.rows.length > 0 && <TableHeader table={table} />}
      <ol className={cn("flex flex-col", table ? "gap-3 md:gap-1.5" : "gap-3")}>
        {state.rows.map((row, index) => {
          const rowId = idOf(row);
          const editable = !locked && (!row.key || row.update === true);
          const removable = !locked && repeater.deletable && (!row.key || row.delete === true);
          const bodyId = `${id}-${rowId}`;
          const failed = Object.keys(server).some((p) => p === rowId || p.startsWith(`${rowId}.`))
            || Object.keys(local).some((p) => p.startsWith(`${index}.`));
          return (
            <li key={rowId} className={cn("rounded-lg border bg-card", table && TABLE_ROW, failed ? "border-destructive" : "border-border")}>
              <div className={cn("flex items-center gap-1 border-b border-border px-2 py-1.5", table && TABLE_BAR, table?.bar)}>
                <span className={cn("min-w-0 flex-1 truncate px-1 text-sm font-medium", table && "md:sr-only")}>{itemLabel(repeater, row.values, index)}</span>
                {repeater.reorderable && !locked && (
                  <>
                    <IconButton label={t("repeater.moveUp")} onClick={() => move(index, index - 1)} disabled={index === 0}><ChevronUp /></IconButton>
                    <IconButton label={t("repeater.moveDown")} onClick={() => move(index, index + 1)} disabled={index === state.rows.length - 1}><ChevronDown /></IconButton>
                  </>
                )}
                {removable && <IconButton label={t("repeater.remove")} onClick={() => remove(index)}><Trash2 /></IconButton>}
              </div>
              <div id={bodyId} className={table ? cn(TABLE_BODY, table.cols) : "grid grid-cols-1 gap-4 p-4 md:grid-cols-2"}>
                {typeof server[rowId] === "string" && <p role="alert" className="col-span-full text-xs text-destructive">{String(server[rowId])}</p>}
                <RowComponents components={row.key ? repeater.components : createView(repeater).components}
                  repeater={row.key ? repeater : createView(repeater)} row={row.values}
                  hidden={hiddenInRow(row.key ? repeater : createView(repeater), row.values, outer)}
                  path={(name) => `${index}.${name}`} problem={problem(index)} server={{}}
                  ctx={{ ...ctx, slug: repeater.relationship?.resource ?? ctx.slug, record: row.key }} disabled={!editable}
                  idPrefix={bodyId} onChange={(next) => setRow(index, next)} />
              </div>
            </li>
          );
        })}
      </ol>
      {typeof listProblem === "string" && <p role="alert" className="text-xs text-destructive">{listProblem}</p>}
      {canCreate && (
        <Button type="button" variant="outline" className="self-start" onClick={add}>
          <Plus /> {repeater.addActionLabel ?? t("repeater.add")}
        </Button>
      )}
    </div>
  );
}
