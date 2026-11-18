import type * as React from "react";
import { cn } from "@/lib/utils";

const COLORS: Record<string, string> = {
  gray: "bg-muted text-foreground",
  primary: "bg-primary/15 text-primary",
  success: "bg-success/15 text-success",
  warning: "bg-warning/15 text-warning",
  danger: "bg-destructive/15 text-destructive",
  info: "border border-border bg-card text-foreground",
};

/** The badge colors `BadgeColumn::color` and `TextEntry::color` document; others show gray. */
const BADGE_COLORS: ReadonlySet<string> = new Set(["primary", "success", "warning", "danger", "gray"]);

/** `color` if it is a documented badge color, else gray. */
export const badgeColor = (color: string | undefined): string => (color && BADGE_COLORS.has(color) ? color : "gray");

export function Badge({ color = "gray", className, ...props }: React.ComponentProps<"span"> & { color?: string }) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-md px-2 py-0.5 text-xs font-medium",
        COLORS[color] ?? COLORS.gray,
        className,
      )}
      {...props}
    />
  );
}
