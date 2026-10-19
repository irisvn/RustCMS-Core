import { createContext, useContext, useId, useRef } from "react";
import type * as React from "react";
import { cn } from "@/lib/utils";

interface TabsState {
  value: string;
  onValueChange: (value: string) => void;
  base: string;
}

const TabsContext = createContext<TabsState | null>(null);

function useTabs(): TabsState {
  const state = useContext(TabsContext);
  if (!state) throw new Error("Tabs parts must be inside <Tabs>");
  return state;
}

const id = (base: string, part: "tab" | "panel", value: string) => `${base}-${part}-${encodeURIComponent(value)}`;

/** Controlled tabs: `value` and `onValueChange` (e.g. from a sub-path). */
export function Tabs({ value, onValueChange, className, children }: { value: string; onValueChange: (value: string) => void; className?: string; children: React.ReactNode }) {
  const base = useId();
  return (
    <TabsContext.Provider value={{ value, onValueChange, base }}>
      <div className={cn("min-w-0", className)}>{children}</div>
    </TabsContext.Provider>
  );
}

/** The tab buttons; arrows, Home and End move and select. */
export function TabsList({ className, children, ...props }: React.ComponentProps<"div"> & { "aria-label": string }) {
  const ref = useRef<HTMLDivElement>(null);
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    props.onKeyDown?.(e);
    const tabs = [...(ref.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]:not([disabled])') ?? [])];
    const current = tabs.indexOf(document.activeElement as HTMLButtonElement);
    if (current < 0) return;
    const keys: Record<string, number> = { ArrowRight: current + 1, ArrowLeft: current - 1, Home: 0, End: tabs.length - 1 };
    if (!(e.key in keys)) return;
    e.preventDefault();
    const next = tabs[(keys[e.key]! + tabs.length) % tabs.length]!;
    next.focus();
    next.click();
  };
  return <div {...props} ref={ref} role="tablist" onKeyDown={onKeyDown} className={cn("flex flex-wrap gap-1 border-b border-border", className)}>{children}</div>;
}

export function TabsTrigger({ value, className, children, disabled }: { value: string; className?: string; children: React.ReactNode; disabled?: boolean }) {
  const tabs = useTabs();
  const selected = tabs.value === value;
  return (
    <button
      type="button"
      role="tab"
      id={id(tabs.base, "tab", value)}
      aria-selected={selected}
      aria-controls={id(tabs.base, "panel", value)}
      tabIndex={selected ? 0 : -1}
      disabled={disabled}
      onClick={() => {
        if (!selected) tabs.onValueChange(value);
      }}
      className={cn(
        "-mb-px inline-flex min-h-9 items-center gap-2 border-b-2 px-3 text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50",
        selected ? "border-foreground text-foreground" : "border-transparent text-muted-foreground hover:text-foreground",
        className,
      )}
    >
      {children}
    </button>
  );
}

/** Rendered only while selected. */
export function TabsContent({ value, className, children }: { value: string; className?: string; children: React.ReactNode }) {
  const tabs = useTabs();
  if (tabs.value !== value) return null;
  return (
    <div role="tabpanel" id={id(tabs.base, "panel", value)} aria-labelledby={id(tabs.base, "tab", value)} tabIndex={0} className={cn("pt-4 focus-visible:outline-none", className)}>
      {children}
    </div>
  );
}
