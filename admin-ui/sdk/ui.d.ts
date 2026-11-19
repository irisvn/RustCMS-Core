// The panel's UI kit (`ui` level 3; level 1 had everything up to `cn`, level 2 up to `CodeBlock`). Props follow the panel's own
// components; they accept the usual HTML attributes of their element.
import type * as React from "react";

type Props<E extends keyof React.JSX.IntrinsicElements> = React.ComponentProps<E> & { className?: string };

export declare const Alert: React.FC<Props<"div"> & { variant?: "default" | "destructive" | "warning" }>;
/** Colors from the panel's tokens; other names show gray (panels up to 0.13.1 have no `info`). */
export declare const Badge: React.FC<Props<"span"> & { color?: "gray" | "primary" | "info" | "success" | "warning" | "danger" }>;
export declare const Button: React.FC<
  Props<"button"> & { variant?: "default" | "outline" | "ghost" | "destructive" | "secondary" | "link"; size?: "default" | "sm" | "lg" | "icon" }
>;
export declare function buttonVariants(options?: { variant?: string; size?: string; className?: string }): string;
export declare const Card: React.FC<Props<"div">>;
export declare const CardContent: React.FC<Props<"div">>;
export declare const CardHeader: React.FC<Props<"div">>;
export declare const CardTitle: React.FC<Props<"h3">>;
export declare const Checkbox: React.FC<Omit<Props<"input">, "type" | "ref"> & { indeterminate?: boolean }>;
export declare const Switch: React.FC<Omit<Props<"button">, "onChange"> & { checked: boolean; onCheckedChange: (checked: boolean) => void }>;
/** A modal; a close button (×) is shown unless `closeButton={false}`; `closeDisabled` blocks it and Escape. */
export declare const Dialog: React.FC<{ open: boolean; onClose: () => void; title: React.ReactNode; children?: React.ReactNode; className?: string; closeButton?: boolean; closeDisabled?: boolean }>;
export declare const Input: React.FC<Props<"input">>;
export declare const Label: React.FC<Props<"label">>;
export declare const NativeSelect: React.FC<Props<"select">>;
export declare const Textarea: React.FC<Props<"textarea">>;
export declare const Page: React.FC<Props<"div">>;
export declare const PageHeader: React.FC<{ title: React.ReactNode; description?: React.ReactNode; actions?: React.ReactNode; eyebrow?: React.ReactNode }>;
export declare const Loading: React.FC<{ label?: string }>;
export declare const ErrorState: React.FC<{ error: unknown }>;
export declare function cn(...inputs: Array<string | false | null | undefined>): string;

// ui 2

/** Controlled tabs (`value` from e.g. `props.path`). */
export declare const Tabs: React.FC<{ value: string; onValueChange: (value: string) => void; className?: string; children: React.ReactNode }>;
export declare const TabsList: React.FC<Props<"div"> & { "aria-label": string }>;
export declare const TabsTrigger: React.FC<{ value: string; className?: string; disabled?: boolean; children: React.ReactNode }>;
export declare const TabsContent: React.FC<{ value: string; className?: string; children: React.ReactNode }>;

/** A menu under a button; `trigger` is the button's content. */
export declare const DropdownMenu: React.FC<{
  trigger: React.ReactNode;
  children: React.ReactNode;
  align?: "start" | "end";
  variant?: "default" | "outline" | "ghost" | "destructive" | "secondary" | "link";
  size?: "default" | "sm" | "lg" | "icon";
  className?: string;
  "aria-label"?: string;
}>;
export declare const DropdownMenuItem: React.FC<{ onSelect?: () => void; destructive?: boolean; disabled?: boolean; children: React.ReactNode }>;
export declare const DropdownMenuSeparator: React.FC;
export declare const DropdownMenuLabel: React.FC<{ children: React.ReactNode }>;

/** `children`: one element that accepts `aria-describedby` and pointer/focus handlers. */
export declare const Tooltip: React.FC<{ content: React.ReactNode; children: React.ReactElement; align?: "start" | "end" | "center" }>;

export declare const Table: React.FC<Props<"table">>;
export declare const TableHeader: React.FC<Props<"thead">>;
export declare const TableBody: React.FC<Props<"tbody">>;
export declare const TableRow: React.FC<Props<"tr">>;
export declare const TableHead: React.FC<Props<"th">>;
export declare const TableCell: React.FC<Props<"td">>;
export declare const Skeleton: React.FC<Props<"div">>;
export declare const Separator: React.FC<{ orientation?: "horizontal" | "vertical"; className?: string }>;
/** Preformatted text with a copy button. */
export declare const CodeBlock: React.FC<{ code: string; label?: string; className?: string }>;


// ui 3

/**
 * Entries drawn as a record's view page draws them (ADR 0039): `schema` is an
 * infolist schema (a panel page's `PageView::Infolist`, or one the page
 * builds), `data` the values by entry name. Relation and computed entries
 * need a record page: here they show their raw value.
 */
export declare const Infolist: React.FC<{ schema: unknown; data: Record<string, unknown>; className?: string }>;
