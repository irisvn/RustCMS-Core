import { useQuery } from "@tanstack/react-query";
import { ChevronsUpDown, X } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { ApiError } from "@/api/client";
import { useApi } from "@/api/context";
import { fetchOptions } from "@/api/queries";
import type { SelectOption } from "@/api/types";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { useT } from "@/i18n";

function useDebounced<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return debounced;
}

const keyOf = (encoded: string) => {
  const v = JSON.parse(encoded) as unknown;
  return typeof v === "string" ? v : String(v);
};

function useSelectedOption(slug: string, field: string, record: string | undefined, value: string, path?: string) {
  const api = useApi();
  return useQuery({
    // The source is part of the key: an action's field may share a resource field's name.
    queryKey: ["options", slug, field, path ?? null, record ?? null, "selected", value],
    queryFn: () => fetchOptions(api, slug, field, { keys: [keyOf(value)], record }, path),
    enabled: value !== "",
    staleTime: 60_000,
  });
}

function selectedLabel(options: SelectOption[] | undefined, value: string) {
  const current = options?.find((o) => JSON.stringify(o.value) === value || keyOf(JSON.stringify(o.value)) === keyOf(value));
  return value === "" ? "" : (current?.label ?? `#${keyOf(value)}`);
}

/** View labels use only the authorized selected-key endpoint, never a search or related record fetch. */
export function RelationValue({ slug, field, record, value }: {
  slug: string; field: string; record: string; value: string;
}) {
  const t = useT();
  const selected = useSelectedOption(slug, field, record, value);
  return <span>
    {selectedLabel(selected.data, value)}
    {selected.isLoading && <span className="ml-2 text-xs text-muted-foreground" role="status">{t("relation.loadingLabel")}</span>}
    {selected.error && <span className="ml-2 text-xs text-muted-foreground">{t("relation.labelUnavailable")}</span>}
  </span>;
}

/**
 * Select over related records, loaded from the field's options endpoint
 * (scoped and authorized by the server; labels the user may not read come back as `#key`).
 * `value` is the JSON encoding of the related key, "" for none.
 */
export function RelationSelect({
  slug, field, path, record, value, onChange, disabled, invalid, id, placeholder, describedBy, onBlur, exclude, onPick,
}: {
  slug: string;
  field: string;
  /** Options endpoint, when not the resource field's. */
  path?: string;
  record?: string;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  invalid?: boolean;
  id?: string;
  placeholder?: string;
  describedBy?: string;
  onBlur?: () => void;
  /** Keys not offered (already chosen elsewhere, e.g. a multi-select's records). */
  exclude?: ReadonlySet<string>;
  /** Called with the chosen option itself (its label), before `onChange`. */
  onPick?: (option: SelectOption) => void;
}) {
  const t = useT();
  const api = useApi();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [active, setActive] = useState(0);
  const debounced = useDebounced(search, 250);
  const listId = useId();
  const box = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const selected = useSelectedOption(slug, field, record, value, path);
  const expanded = open && !disabled;
  const results = useQuery({
    queryKey: ["options", slug, field, path ?? null, record ?? null, "search", debounced],
    queryFn: () => fetchOptions(api, slug, field, { search: debounced || undefined, record }, path),
    enabled: expanded,
  });
  // Clear is a keyboard-selectable option too. Never choose stale results while a search is changing.
  const options: (SelectOption | null)[] = [
    ...(value !== "" ? [null] : []),
    ...(search === debounced ? (results.data ?? []).filter((o) => !exclude?.has(keyOf(JSON.stringify(o.value)))) : []),
  ];
  const activeIndex = options.length ? Math.min(active, options.length - 1) : -1;
  const activeId = activeIndex >= 0 ? `${listId}-option-${activeIndex}` : undefined;

  useEffect(() => {
    if (!expanded) return;
    const close = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [expanded]);

  useEffect(() => {
    if (expanded && activeId) document.getElementById(activeId)?.scrollIntoView?.({ block: "nearest" });
  }, [expanded, activeId]);

  const close = (restoreFocus: boolean) => {
    setOpen(false);
    setSearch("");
    if (restoreFocus) trigger.current?.focus();
  };
  const choose = (option: SelectOption | null) => {
    if (option) onPick?.(option);
    onChange(option ? JSON.stringify(option.value) : "");
    close(true);
  };
  const keyDown = (e: KeyboardEvent<HTMLElement>) => {
    // IME candidate navigation/confirmation belongs to the text editor, not the listbox.
    // Some browsers report the final composition key only through legacy keyCode 229.
    if (e.nativeEvent.isComposing || e.nativeEvent.keyCode === 229) return;
    if (e.key === "Escape" && expanded) {
      e.preventDefault();
      e.stopPropagation();
      close(true);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!expanded) {
        setActive(0);
        setOpen(true);
      } else if (options.length) {
        setActive((activeIndex + (e.key === "ArrowDown" ? 1 : -1) + options.length) % options.length);
      }
    } else if (e.key === "Enter" && expanded) {
      e.preventDefault();
      if (activeIndex >= 0) choose(options[activeIndex]!);
    }
  };

  return (
    <div ref={box} className="relative" onBlur={(e) => {
      if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
        close(false);
        onBlur?.();
      }
    }}>
      <button
        ref={trigger}
        id={id}
        type="button"
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={expanded}
        aria-controls={listId}
        aria-invalid={invalid || undefined}
        aria-describedby={describedBy}
        disabled={disabled}
        onKeyDown={keyDown}
        onClick={() => {
          if (expanded) close(false);
          else { setActive(0); setOpen(true); }
        }}
        className={cn(
          "flex h-9 w-full items-center justify-between gap-2 rounded-md border border-input bg-card px-3 text-left text-sm shadow-xs transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60",
          invalid && "border-destructive",
        )}
      >
        <span className={cn("truncate", !value && "text-muted-foreground")}>{selectedLabel(selected.data, value) || placeholder || t("relation.select")}</span>
        <ChevronsUpDown className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      </button>
      {expanded && (
        <div className="absolute z-20 mt-2 w-full rounded-lg border border-border bg-card p-1.5 shadow-lg">
          <Input
            autoFocus role="combobox" aria-label={t("relation.searchOptions")} aria-expanded="true"
            aria-controls={listId} aria-autocomplete="list" aria-activedescendant={activeId}
            aria-invalid={invalid || undefined} aria-describedby={describedBy}
            placeholder={t("relation.searchPlaceholder")} value={search} onKeyDown={keyDown}
            onChange={(e) => { setSearch(e.target.value); setActive(0); }} className="mb-1.5"
          />
          <ul id={listId} role="listbox" aria-label={t("relation.options")} className="max-h-60 overflow-auto">
            {options.map((option, index) => {
              const encoded = option ? JSON.stringify(option.value) : "";
              return (
                <li key={option ? encoded : "clear"} id={`${listId}-option-${index}`} role="option" aria-selected={option ? encoded === value : false}>
                  <button
                    type="button" tabIndex={-1}
                    className={cn("flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-sm hover:bg-muted", index === activeIndex && "bg-muted", encoded === value && "font-medium", !option && "text-muted-foreground")}
                    onMouseDown={(e) => e.preventDefault()}
                    onMouseEnter={() => setActive(index)}
                    onClick={() => choose(option)}
                  >
                    {option ? option.label : <><X className="size-3" aria-hidden="true" /> {t("relation.clear")}</>}
                  </button>
                </li>
              );
            })}
          </ul>
          {(results.isLoading || search !== debounced) && <p role="status" className="px-2.5 py-2 text-sm text-muted-foreground">{t("state.loading")}</p>}
          {results.error && (
            <p role="alert" className="px-2.5 py-2 text-sm text-destructive">
              {results.error instanceof ApiError && results.error.status === 422
                ? (Object.values(results.error.fieldErrors)[0]?.[0] ?? t("relation.refine"))
                : t("relation.loadFailed")}
            </p>
          )}
          {results.data && !options.some((o) => o !== null) && search === debounced && <p role="status" className="px-2.5 py-2 text-sm text-muted-foreground">{t("list.noResults")}</p>}
        </div>
      )}
    </div>
  );
}
