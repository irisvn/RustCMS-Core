// A repeater (ADR 0040): rows of inputs, added, removed, copied, folded and
// reordered (buttons, or dragging the handle). Each row input is drawn by the
// form's own widgets; the rows live in the form as one JSON value.
import { ChevronDown, ChevronUp, Copy, GripVertical, Plus, Trash2 } from "lucide-react";
import { Fragment, useRef, useState } from "react";
import type * as React from "react";
import type { ComponentSchema, InputSchema, RepeaterSchema } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/input";
import { useT } from "@/i18n";
import { cn } from "@/lib/utils";
import type { Outer } from "@/schema/conditions";
import { inputsOf, isInput, isLayout, isTriState } from "@/schema/form";
import { type Row, hiddenInRow, itemLabel, parseRows, rowLevel, rowOf, rowProblems } from "@/schema/repeater";
import { OuterContext, useOuter } from "./outer";
import { type FormContext, SchemaLayout, Widget, type WidgetProps, span } from "./form";

let nextKey = 0;
const newKey = () => `row-${++nextKey}`;

/** Stable React keys for rows changed only through this widget (reset when the value is replaced). */
/** `problems` by path with the row index they name moved to where that row is now (gone: dropped). */
export function followRows(problems: Record<string, unknown>, then: string[], now: string[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [path, problem] of Object.entries(problems)) {
    const [head, ...rest] = path.split(".");
    if (!/^\d+$/.test(head!)) {
      out[path] = problem;
      continue;
    }
    const at = now.indexOf(then[Number(head)] ?? "");
    if (at >= 0) out[[String(at), ...rest].join(".")] = problem;
  }
  return out;
}

function useRowKeys(count: number) {
  const keys = useRef<string[]>([]);
  if (keys.current.length !== count) keys.current = Array.from({ length: count }, newKey);
  return keys;
}

export function RepeaterWidget({ id, input, ctx, value, onChange, onBlur, disabled, invalid, describedBy }: WidgetProps) {
  const t = useT();
  const repeater = input.repeater;
  const outer = useOuter();
  const rows = parseRows(value);
  const keys = useRowKeys(rows?.length ?? 0);
  const [folded, setFolded] = useState<Set<string>>(() => (repeater?.collapsed ? new Set(keys.current) : new Set()));
  const dragged = useRef<number | null>(null);
  // The server names rows by their index at the refused save: remember which
  // rows those were, so its problems follow them when rows move or go away.
  const answer = ctx.errors[input.name];
  const answered = useRef<{ answer: unknown; keys: string[] }>({ answer: undefined, keys: [] });
  if (answered.current.answer !== answer) answered.current = { answer, keys: [...keys.current] };
  if (!repeater || rows === null) return <p className="text-sm text-muted-foreground">{t("form.json")}</p>;

  // Problems by path: the server's (after a refused save), else the panel's once the user tried to save.
  const server = followRows((answer as { types?: Record<string, unknown> } | undefined)?.types ?? {}, answered.current.keys, keys.current);
  const local = ctx.submitCount ? rowProblems(repeater, rows, "", outer) : {};
  const problem = (path: string) => {
    const found = server[path] ?? local[path];
    return Array.isArray(found) ? found.join(" ") : typeof found === "string" ? found : undefined;
  };

  const max = repeater.maxItems;
  const canAdd = !disabled && repeater.addable && (max === undefined || rows.length < max);
  const canRemove = !disabled && repeater.deletable;
  const commit = (next: Row[], nextKeys: string[]) => {
    keys.current = nextKeys;
    onChange(JSON.stringify(next));
  };
  const add = () => commit([...rows, rowOf(repeater, {})], [...keys.current, newKey()]);
  const remove = (index: number) => commit(rows.filter((_, i) => i !== index), keys.current.filter((_, i) => i !== index));
  const clone = (index: number) => {
    const next = [...rows];
    next.splice(index + 1, 0, structuredClone(rows[index]!));
    const nextKeys = [...keys.current];
    nextKeys.splice(index + 1, 0, newKey());
    commit(next, nextKeys);
  };
  const move = (from: number, to: number) => {
    if (to < 0 || to >= rows.length || from === to) return;
    const next = [...rows];
    const nextKeys = [...keys.current];
    next.splice(to, 0, next.splice(from, 1)[0]!);
    nextKeys.splice(to, 0, nextKeys.splice(from, 1)[0]!);
    commit(next, nextKeys);
  };
  const setRow = (index: number, row: Row) => commit(rows.map((r, i) => (i === index ? row : r)), keys.current);
  const toggle = (key: string) =>
    setFolded((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const listProblem = problem("");
  const table = tableOf(repeater, (repeater.reorderable && !disabled ? 3 : 0) + (repeater.cloneable && canAdd ? 1 : 0) + (canRemove ? 1 : 0));
  return (
    <div id={id} className="flex flex-col gap-3" aria-describedby={describedBy} aria-invalid={invalid || undefined} onBlur={onBlur}>
      {rows.length === 0 && <p className="text-sm text-muted-foreground">{t("repeater.empty")}</p>}
      {table && rows.length > 0 && <TableHeader table={table} />}
      <ol className={cn("flex flex-col", table ? "gap-3 md:gap-1.5" : "gap-3")}>
        {rows.map((row, index) => {
          const key = keys.current[index]!;
          const label = itemLabel(repeater, row, index);
          const isFolded = repeater.collapsible && folded.has(key);
          const bodyId = `${id}-${key}`;
          const rowHasProblem = Object.keys({ ...server, ...local }).some((p) => p === String(index) || p.startsWith(`${index}.`));
          return (
            <li
              key={key}
              className={cn("rounded-lg border bg-card", table && TABLE_ROW, rowHasProblem ? "border-destructive" : "border-border")}
              onDragOver={(e) => {
                if (dragged.current !== null) e.preventDefault();
              }}
              onDrop={(e) => {
                e.preventDefault();
                if (dragged.current !== null) move(dragged.current, index);
                dragged.current = null;
              }}
            >
              <div className={cn("flex items-center gap-1 border-b border-border px-2 py-1.5", table && TABLE_BAR, table?.bar)}>
                {repeater.reorderable && !disabled && (
                  <span
                    draggable
                    onDragStart={() => {
                      dragged.current = index;
                    }}
                    onDragEnd={() => {
                      dragged.current = null;
                    }}
                    title={t("repeater.drag")}
                    className="cursor-grab text-muted-foreground"
                  >
                    <GripVertical className="size-4" aria-hidden="true" />
                  </span>
                )}
                {repeater.collapsible ? (
                  <button type="button" onClick={() => toggle(key)} aria-expanded={!isFolded} aria-controls={bodyId}
                    className="flex min-w-0 flex-1 items-center gap-1.5 rounded px-1 text-left text-sm font-medium hover:bg-muted">
                    <ChevronDown className={cn("size-4 shrink-0 transition-transform", isFolded && "-rotate-90")} aria-hidden="true" />
                    <span className="truncate">{label}</span>
                  </button>
                ) : (
                  <span className={cn("min-w-0 flex-1 truncate px-1 text-sm font-medium", table && "md:sr-only")}>{label}</span>
                )}
                {repeater.reorderable && !disabled && (
                  <>
                    <IconButton label={t("repeater.moveUp")} onClick={() => move(index, index - 1)} disabled={index === 0}><ChevronUp /></IconButton>
                    <IconButton label={t("repeater.moveDown")} onClick={() => move(index, index + 1)} disabled={index === rows.length - 1}><ChevronDown /></IconButton>
                  </>
                )}
                {repeater.cloneable && canAdd && <IconButton label={t("repeater.clone")} onClick={() => clone(index)}><Copy /></IconButton>}
                {canRemove && <IconButton label={t("repeater.remove")} onClick={() => remove(index)}><Trash2 /></IconButton>}
              </div>
              <div id={bodyId} hidden={isFolded} className={table ? cn(TABLE_BODY, table.cols) : "grid grid-cols-1 gap-4 p-4 md:grid-cols-2"}>
                {problem(String(index)) && <p role="alert" className="col-span-full text-xs text-destructive">{problem(String(index))}</p>}
                {(ctx.rowFlags?.[input.name]?.[index] ?? []).map((flag) => (
                  <p key={flag} role="status" className="col-span-full text-xs text-warning">{flag}</p>
                ))}
                <NestedRow repeater={repeater} row={row} outer={outer}>
                  {(hidden) => (
                    <RowComponents components={repeater.components} repeater={repeater} row={row} hidden={hidden}
                      path={(name) => `${index}.${name}`} problem={problem} server={server} ctx={ctx} disabled={disabled}
                      idPrefix={bodyId} onChange={(next) => setRow(index, next)} />
                  )}
                </NestedRow>
              </div>
            </li>
          );
        })}
      </ol>
      {listProblem && <p role="alert" className="text-xs text-destructive">{listProblem}</p>}
      {canAdd && (
        <Button type="button" variant="outline" className="self-start" onClick={add}>
          <Plus /> {repeater.addActionLabel ?? t("repeater.add")}
        </Button>
      )}
    </div>
  );
}

/**
 * A repeater laid out as a table (ADR 0044): from `md`, one column per input
 * (labels in a header, kept for screen readers in each cell) and the row's
 * buttons last; below, stacked rows. One set of inputs either way.
 */
export interface RepeaterTable {
  labels: { name: string; label: string; required: boolean }[];
  /** Classes of the input columns (from `md`) and of the buttons' width: no inline style (CSP). */
  cols: string;
  bar: string;
}

// Written out for Tailwind: equal columns (more inputs wrap at 6), and the
// buttons' width (1.75rem each, 0.25rem gaps, 0.5rem padding on each side).
const TABLE_COLS = ["md:grid-cols-1", "md:grid-cols-2", "md:grid-cols-3", "md:grid-cols-4", "md:grid-cols-5", "md:grid-cols-6"];
const TABLE_BARS = ["md:w-[2.75rem]", "md:w-[4.75rem]", "md:w-[6.75rem]", "md:w-[8.75rem]", "md:w-[10.75rem]"];

/** The table layout of `repeater` with `buttons` buttons per row, or null for stacked rows. */
export function tableOf(repeater: RepeaterSchema, buttons: number): RepeaterTable | null {
  if (repeater.layout !== "table") return null;
  const inputs = inputsOf(repeater.components).filter((i) => i.type !== "hidden");
  return {
    labels: inputs.map((i) => ({ name: i.name, label: i.label, required: i.rules.some((r) => r.rule === "required") && !i.disabled })),
    cols: TABLE_COLS[Math.min(Math.max(inputs.length, 1), TABLE_COLS.length) - 1]!,
    bar: TABLE_BARS[Math.min(Math.max(buttons, 1), TABLE_BARS.length) - 1]!,
  };
}

export const TABLE_ROW = "md:flex md:flex-row md:items-stretch md:rounded-md";
export const TABLE_BAR = "md:order-last md:shrink-0 md:justify-end md:border-b-0 md:border-l";
export const TABLE_BODY = "grid grid-cols-1 gap-4 p-4 md:flex-1 md:gap-3 md:p-2";

/** The header of a table layout: the inputs' labels, for sighted users (cells keep theirs). */
export function TableHeader({ table }: { table: RepeaterTable }) {
  return (
    <div aria-hidden="true" className="hidden md:flex">
      <div className={cn("grid flex-1 gap-3 px-2 text-sm font-medium", table.cols)}>
        {table.labels.map((l) => (
          <span key={l.name} className="truncate">{l.label}{l.required && <span className="text-destructive"> *</span>}</span>
        ))}
      </div>
      <div className={cn("shrink-0", table.bar)} />
    </div>
  );
}

export function IconButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: React.ReactNode }) {
  return (
    <Button type="button" variant="ghost" size="icon" className="size-7 [&_svg]:size-4" aria-label={label} title={label} onClick={onClick} disabled={disabled}>
      {children}
    </Button>
  );
}

/**
 * A row's hidden inputs (its conditions read the row and `outer`), and the
 * row as the repeaters nested in it read it (`when_parent`).
 */
export function NestedRow({ repeater, row, outer, children }: {
  repeater: RepeaterSchema;
  row: Row;
  outer: Outer;
  children: (hidden: Set<string>) => React.ReactNode;
}) {
  const hidden = hiddenInRow(repeater, row, outer);
  return <OuterContext.Provider value={{ parent: rowLevel(repeater, row, hidden), root: outer.root }}>{children(hidden)}</OuterContext.Provider>;
}

export function RowComponents({ components, repeater, row, hidden, path, problem, server, ctx, disabled, idPrefix, onChange, hiddenNotice }: {
  components: ComponentSchema[];
  repeater: RepeaterSchema;
  row: Row;
  hidden: Set<string>;
  path: (name: string) => string;
  problem: (path: string) => string | undefined;
  server: Record<string, unknown>;
  ctx: FormContext;
  disabled?: boolean;
  idPrefix: string;
  onChange: (row: Row) => void;
  /** In place of a hidden relationship repeater: what its rows' removal shows (ADR 0044). */
  hiddenNotice?: (input: InputSchema) => React.ReactNode;
}) {
  return <>{components.map((component, index) => {
    if (isLayout(component)) {
      return (
        <SchemaLayout key={`layout-${index}`} layout={component}>
          <RowComponents components={component.components} repeater={repeater} row={row} hidden={hidden} path={path}
            problem={problem} server={server} ctx={ctx} disabled={disabled} idPrefix={idPrefix} onChange={onChange}
            hiddenNotice={hiddenNotice} />
        </SchemaLayout>
      );
    }
    const table = repeater.layout === "table";
    if (isInput(component) && hidden.has(component.name) && component.repeater?.relationship) {
      return <Fragment key={component.name}>{hiddenNotice?.(component) ?? null}</Fragment>;
    }
    // A hidden input keeps its column in a table row: an empty cell.
    if (isInput(component) && hidden.has(component.name) && table && component.type !== "hidden") {
      return <div key={component.name} aria-hidden="true" className="hidden md:block" />;
    }
    if (!isInput(component) || hidden.has(component.name)) return null;
    return <RowField key={component.name} input={component} row={row} path={path(component.name)} problem={problem}
      server={server} ctx={ctx} disabled={disabled} id={`${idPrefix}-${component.name}`} onChange={onChange} table={table} />;
  })}</>;
}

function RowField({ input, row, path, problem, server, ctx, disabled, id, onChange, table }: {
  /** In a table row: the label is the header's (kept for screen readers). */
  table?: boolean;
  input: InputSchema;
  row: Row;
  path: string;
  problem: (path: string) => string | undefined;
  server: Record<string, unknown>;
  ctx: FormContext;
  disabled?: boolean;
  id: string;
  onChange: (row: Row) => void;
}) {
  if (input.type === "hidden") return null;
  const error = problem(path);
  const off = disabled || input.disabled;
  const required = input.rules.some((r) => r.rule === "required");
  const describedBy = [input.help && `${id}-help`, error && `${id}-error`].filter(Boolean).join(" ") || undefined;
  // A nested repeater shows its rows' problems itself, below its own path.
  // Reactions flag rows of the form's own repeaters only.
  // Uploads in a row block saving by row (its input's id), not by input name (ADR 0044).
  const rowCtx: FormContext = ctx.onBusy ? { ...ctx, onBusy: (_name, busy) => ctx.onBusy?.(id, busy) } : ctx;
  const nestedCtx: FormContext =
    input.type === "repeater" ? { ...rowCtx, errors: nestedErrors(input.name, path, server), rowFlags: undefined } : rowCtx;
  return (
    <div className={cn("flex min-w-0 flex-col gap-2", !table && span(input.columnSpan))}>
      {(input.type !== "checkbox" || isTriState(input)) && (
        <Label htmlFor={id} className={cn(table && "md:sr-only")}>
          {input.label}
          {required && !off && <span className="text-destructive"> *</span>}
        </Label>
      )}
      <Widget id={id} input={input} ctx={nestedCtx} value={row[input.name] ?? ""} disabled={off} invalid={!!error}
        describedBy={describedBy} onBlur={() => {}} onChange={(value) => onChange({ ...row, [input.name]: value })} />
      {input.help && <p id={`${id}-help`} className={cn("text-xs leading-5 text-muted-foreground", table && "md:sr-only")}>{input.help}</p>}
      {error && input.type !== "repeater" && <p id={`${id}-error`} role="alert" className="text-xs leading-5 text-destructive">{error}</p>}
    </div>
  );
}

/** The errors a nested repeater reads: the server's problems below `path`, as `types` of its name. */
function nestedErrors(name: string, path: string, server: Record<string, unknown>): FormContext["errors"] {
  const types: Record<string, unknown> = {};
  for (const [key, message] of Object.entries(server)) {
    if (key === path) types[""] = message;
    else if (key.startsWith(`${path}.`)) types[key.slice(path.length + 1)] = message;
  }
  return { [name]: { type: "server", types } } as unknown as FormContext["errors"];
}
