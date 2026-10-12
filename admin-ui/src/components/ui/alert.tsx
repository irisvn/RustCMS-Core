import type * as React from "react";
import { cn } from "@/lib/utils";

export function Alert({
  variant = "default",
  className,
  ...props
}: React.ComponentProps<"div"> & { variant?: "default" | "destructive" | "warning" }) {
  return (
    <div
      role="alert"
      className={cn(
        "rounded-md border px-4 py-3 text-sm",
        variant === "destructive" && "border-destructive/40 bg-destructive/10 text-destructive",
        variant === "warning" && "border-warning/40 bg-warning/10 text-warning",
        variant === "default" && "border-border bg-card",
        className,
      )}
      {...props}
    />
  );
}
