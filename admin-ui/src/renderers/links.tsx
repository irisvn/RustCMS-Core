// The links of a belongs-to-many relation, chosen in the record's own form
// (ADR 0035). The form holds a change, never the list: `{attach, detach}` of
// target keys ("" for none), so links made by others meanwhile, and links the
// user cannot see, are never undone by a save. The server checks every key and
// ability again when the record is saved.
import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ApiError } from "@/api/client";
import { formLinkOptionsPath, useFormLinks } from "@/api/queries";
import type { InputSchema } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { useT } from "@/i18n";
import { RelationSelect } from "./relation-select";

/**
 * Most current links the input edits (more are changed on the record's page),
 * and most records one save attaches (the server's limit).
 */
export const MAX_FORM_LINKS = 100;

export interface LinkChange {
  attach: string[];
  detach: string[];
}

/** The change a form value holds ("" is none). */
export function parseLinks(value: unknown): LinkChange {
  if (typeof value !== "string" || value === "") return { attach: [], detach: [] };
  const parsed = JSON.parse(value) as Partial<LinkChange>;
  return { attach: parsed.attach ?? [], detach: parsed.detach ?? [] };
}

/** The form value of a change: "" when it changes nothing. */
export function linksValue(change: LinkChange): string {
  const detach = [...new Set(change.detach)].sort();
  // Never both for one key (the server refuses that).
  const attach = [...new Set(change.attach)].filter((k) => !detach.includes(k)).sort();
  if (attach.length === 0 && detach.length === 0) return "";
  return JSON.stringify({ ...(attach.length ? { attach } : {}), ...(detach.length ? { detach } : {}) });
}

/**
 * Takes `key` out of what the input shows. The change stays disjoint whatever
 * the current list says meanwhile (another user may link a record chosen
 * here): a key is attached or detached, never both.
 */
export function removeLink(change: LinkChange, key: string, linked: ReadonlySet<string>, canDetach: boolean): LinkChange {
  const attach = change.attach.filter((k) => k !== key);
  const pending = attach.length !== change.attach.length;
  const detach = linked.has(key) && (!pending || canDetach) && !change.detach.includes(key) ? [...change.detach, key] : change.detach;
  return { attach, detach };
}

/** Puts `key` (back) into what the input shows. */
export function addLink(change: LinkChange, key: string, linked: ReadonlySet<string>): LinkChange {
  const detach = change.detach.filter((k) => k !== key);
  const attach = linked.has(key) || change.attach.includes(key) ? change.attach : [...change.attach, key];
  return { attach, detach };
}

export function LinksWidget({ id, input, slug, record, value, onChange, disabled, invalid, describedBy, onBlur }: {
  id: string;
  input: InputSchema;
  slug: string;
  /** The edited record (none on create). */
  record?: string;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  invalid: boolean;
  describedBy?: string;
  onBlur: () => void;
}) {
  const t = useT();
  const target = input.relationship?.resource ?? "";
  const abilities = input.links ?? { attach: false, detach: false };
  const via = { parent: slug, record: record ?? "", relation: input.name };
  const current = useFormLinks(target, via, MAX_FORM_LINKS, record !== undefined);
  // Labels of the records chosen here (the current ones come with the list).
  // Maps, not objects: a key may be any text (`__proto__` included).
  const [picked, setPicked] = useState<ReadonlyMap<string, string>>(new Map());
  const change = parseLinks(value);
  const rows = current.data?.data ?? [];
  const tooMany = (current.data?.meta.total ?? 0) > rows.length;
  // Whether the current links are known (a blind editor, who may update but
  // not view the record, cannot list them, yet may still attach).
  const known = record === undefined || current.data !== undefined;
  const blind = current.error instanceof ApiError && current.error.status === 403;
  const detached = new Set(change.detach);
  const labels = new Map(picked);
  for (const row of rows) labels.set(row.key, row.title ?? `#${row.key}`);
  const shown = [...rows.map((r) => r.key).filter((k) => !detached.has(k)), ...change.attach.filter((k) => !rows.some((r) => r.key === k))];
  const linked = new Set(rows.map((r) => r.key));
  const locked = disabled || tooMany;
  const box = useRef<HTMLDivElement>(null);
  // The removed chip's button is gone: once rendered, focus goes to the
  // search, else to the input itself.
  const [moved, setMoved] = useState(0);
  useEffect(() => {
    if (moved === 0) return;
    const search = document.getElementById(id);
    (search && box.current?.contains(search) ? search : box.current)?.focus();
  }, [moved, id]);
  const refocus = () => setMoved((n) => n + 1);
  const remove = (key: string) => {
    onChange(linksValue(removeLink(change, key, linked, abilities.detach)));
    refocus();
  };
  const add = (key: string) => onChange(linksValue(addLink(change, key, linked)));
  const searching = abilities.attach && !locked && change.attach.length < MAX_FORM_LINKS;
  return (
    // Without the search, the input itself carries the field's id (label,
    // help, errors and focus after a refused save).
    <div ref={box} className="flex min-w-0 flex-col gap-2 rounded-md focus-visible:outline-none"
      {...(searching ? {} : { id, tabIndex: -1, role: "group", "aria-label": input.label, "aria-describedby": describedBy, "aria-invalid": invalid || undefined })}>
      {record !== undefined && current.isLoading && <p role="status" className="text-sm text-muted-foreground">{t("state.loading")}</p>}
      {current.error && (blind
        ? <p className="text-sm text-muted-foreground">{t("links.blind")}</p>
        : <p role="alert" className="text-sm text-destructive">{t("links.loadFailed")}</p>)}
      {tooMany && <p className="text-sm text-muted-foreground">{t("links.tooMany", { max: rows.length })}</p>}
      {shown.length === 0 && known && <p className="text-sm text-muted-foreground">{t("links.none")}</p>}
      {shown.length > 0 && (
        <ul aria-label={input.label} className="flex flex-wrap gap-2">
          {shown.map((key) => {
            const label = labels.get(key) ?? `#${key}`;
            // Current links go only with `detach`; records chosen here can always be put back.
            const removable = !locked && (!linked.has(key) || abilities.detach || change.attach.includes(key));
            return (
              <li key={key}>
                <Badge className="gap-1 pr-1">
                  {label}
                  {removable && (
                    <button type="button" aria-label={t("links.remove", { label })} onClick={() => remove(key)}
                      className="inline-flex size-5 items-center justify-center rounded hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                      <X className="size-3" aria-hidden="true" />
                    </button>
                  )}
                </Badge>
              </li>
            );
          })}
        </ul>
      )}
      {/* Removed here, not saved yet: put back without needing `attach`. */}
      {rows.some((r) => detached.has(r.key)) && (
        <ul aria-label={t("links.removed")} className="flex flex-wrap items-center gap-2">
          {rows.filter((r) => detached.has(r.key)).map((row) => {
            const label = labels.get(row.key) ?? `#${row.key}`;
            return (
              <li key={row.key} className="flex items-center gap-1 text-sm text-muted-foreground">
                <s>{label}</s>
                {!locked && (
                  <button type="button" aria-label={t("links.keep", { label })} onClick={() => { add(row.key); refocus(); }}
                    className="rounded px-1 text-xs underline hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                    {t("links.undo")}
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {abilities.attach && !locked && change.attach.length >= MAX_FORM_LINKS && (
        <p className="text-sm text-muted-foreground">{t("links.full", { max: MAX_FORM_LINKS })}</p>
      )}
      {searching && (
        <RelationSelect
          id={id}
          slug={slug}
          field={input.name}
          path={formLinkOptionsPath(slug, input.name, record)}
          value=""
          exclude={new Set(shown)}
          onPick={(option) => setPicked((labels) => new Map(labels).set(String(option.value), option.label))}
          onChange={(encoded) => {
            if (encoded !== "") add(String(JSON.parse(encoded)));
          }}
          invalid={invalid}
          describedBy={describedBy}
          onBlur={onBlur}
          placeholder={input.placeholder ?? t("links.add")}
        />
      )}
    </div>
  );
}
