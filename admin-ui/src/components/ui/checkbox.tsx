import type * as React from "react";
import { cn } from "@/lib/utils";

export function Checkbox({ className, indeterminate = false, ...props }: Omit<React.ComponentProps<"input">, "type" | "ref"> & { indeterminate?: boolean }) {
  return (
    <input
      type="checkbox"
      ref={(node) => { if (node) node.indeterminate = indeterminate; }}
      aria-checked={indeterminate ? "mixed" : undefined}
      className={cn("size-4 shrink-0 rounded border-input accent-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:opacity-60", className)}
      {...props}
    />
  );
}

export function Switch({
  checked,
  onCheckedChange,
  disabled,
  className,
  ...props
}: Omit<React.ComponentProps<"button">, "onChange"> & {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onCheckedChange(!checked)}
      className={cn(
        "inline-flex h-5 w-9 shrink-0 items-center rounded-full border-2 border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60",
        checked ? "bg-primary" : "bg-secondary",
        className,
      )}
      {...props}
    >
      <span
        className={cn(
          "block size-4 rounded-full shadow transition-transform", checked ? "bg-primary-foreground" : "bg-card",
          checked ? "translate-x-4" : "translate-x-0",
        )}
      />
    </button>
  );
}
