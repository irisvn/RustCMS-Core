import type * as React from "react";
import { cn } from "@/lib/utils";

const COLORS: Record<string, string> = {
  gray: "bg-gray-soft text-gray-soft-foreground",
  // Solid: the default primary is neutral, a soft one would read as gray.
  primary: "bg-primary text-primary-foreground",
  info: "bg-info-soft text-info-soft-foreground",
  success: "bg-success-soft text-success-soft-foreground",
  warning: "bg-warning-soft text-warning-soft-foreground",
  danger: "bg-destructive-soft text-destructive-soft-foreground",
};

/** The badge colors `BadgeColumn::color` and `TextEntry::color` document; others show gray. */
const BADGE_COLORS: ReadonlySet<string> = new Set(["primary", "info", "success", "warning", "danger", "gray"]);

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
