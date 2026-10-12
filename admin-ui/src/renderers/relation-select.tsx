import { useQuery } from "@tanstack/react-query";
import { ChevronsUpDown, X } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { ApiError } from "@/api/client";
import { useApi } from "@/api/context";
import { fetchOptions } from "@/api/queries";
import type { SelectOption } from "@/api/types";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

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

/**
 * Select over related records, loaded from the field's options endpoint
 * (scoped and authorized by the server; labels the user may not read come back as `#key`).
 * `value` is the JSON encoding of the related key, "" for none.
 */
export function RelationSelect({
  slug,
  field,
  record,
  value,
  onChange,
  disabled,
  invalid,
  id,
  placeholder,
}: {
  slug: string;
  field: string;
  record?: string;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  invalid?: boolean;
  id?: string;
  placeholder?: string;
}) {
  const api = useApi();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const debounced = useDebounced(search, 250);
  const listId = useId();
  const box = useRef<HTMLDivElement>(null);

  const selected = useQuery({
    queryKey: ["options", slug, field, record ?? null, "selected", value],
    queryFn: () => fetchOptions(api, slug, field, { keys: [keyOf(value)], record }),
    enabled: value !== "",
    staleTime: 60_000,
  });
  const results = useQuery({
    queryKey: ["options", slug, field, record ?? null, "search", debounced],
    queryFn: () => fetchOptions(api, slug, field, { search: debounced || undefined, record }),
    enabled: open,
  });

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  const current = selected.data?.find((o) => JSON.stringify(o.value) === value || keyOf(JSON.stringify(o.value)) === keyOf(value));
  const label = value === "" ? "" : (current?.label ?? `#${keyOf(value)}`);
  const choose = (option: SelectOption | null) => {
    onChange(option ? JSON.stringify(option.value) : "");
    setOpen(false);
    setSearch("");
  };

  return (
    <div ref={box} className="relative">
      <button
        id={id}
        type="button"
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-invalid={invalid || undefined}
        disabled={disabled}
        onClick={() => setOpen((o) => !o)}
        className={cn(
          "flex h-9 w-full items-center justify-between rounded-md border border-input bg-card px-3 text-left text-sm shadow-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60",
          invalid && "border-destructive",
        )}
      >
        <span className={cn("truncate", !label && "text-muted-foreground")}>{label || placeholder || "Select…"}</span>
        <ChevronsUpDown className="size-4 opacity-50" />
      </button>
      {open && (
        <div className="absolute z-20 mt-1 w-full rounded-md border border-border bg-card p-1 shadow-lg">
          <Input autoFocus placeholder="Search…" value={search} onChange={(e) => setSearch(e.target.value)} className="mb-1" />
          <ul id={listId} role="listbox" className="max-h-60 overflow-auto">
            {value !== "" && (
              <li>
                <button type="button" className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-sm text-muted-foreground hover:bg-muted" onClick={() => choose(null)}>
                  <X className="size-3" /> Clear
                </button>
              </li>
            )}
            {results.isLoading && <li className="px-2 py-1.5 text-sm text-muted-foreground">Loading…</li>}
            {results.error && (
              <li className="px-2 py-1.5 text-sm text-destructive">
                {results.error instanceof ApiError && results.error.status === 422
                  ? (Object.values(results.error.fieldErrors)[0]?.[0] ?? "Refine the search.")
                  : "Could not load options."}
              </li>
            )}
            {results.data?.length === 0 && <li className="px-2 py-1.5 text-sm text-muted-foreground">No results</li>}
            {results.data?.map((option) => {
              const encoded = JSON.stringify(option.value);
              return (
                <li key={encoded} role="option" aria-selected={encoded === value}>
                  <button
                    type="button"
                    className={cn("w-full rounded px-2 py-1.5 text-left text-sm hover:bg-muted", encoded === value && "bg-muted font-medium")}
                    onClick={() => choose(option)}
                  >
                    {option.label}
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </div>
  );
}
