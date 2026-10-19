import { Check, Copy } from "lucide-react";
import { useState } from "react";
import type * as React from "react";
import { useT } from "@/i18n";
import { cn } from "@/lib/utils";

/** A loading placeholder. */
export function Skeleton({ className, ...props }: React.ComponentProps<"div">) {
  return <div aria-hidden="true" {...props} className={cn("animate-pulse rounded-md bg-muted", className)} />;
}

export function Separator({ orientation = "horizontal", className }: { orientation?: "horizontal" | "vertical"; className?: string }) {
  return (
    <div
      role="separator"
      aria-orientation={orientation}
      className={cn("shrink-0 bg-border", orientation === "horizontal" ? "h-px w-full" : "h-full w-px", className)}
    />
  );
}

/** Preformatted text with a copy button (no highlighting). */
export function CodeBlock({ code, className, label }: { code: string; className?: string; label?: string }) {
  const t = useT();
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const copy = async () => {
    try {
      if (!navigator.clipboard) throw new Error("no clipboard");
      await navigator.clipboard.writeText(code);
      setState("copied");
    } catch {
      setState("failed");
    }
  };
  return (
    <div className={cn("relative min-w-0 rounded-lg border border-border bg-muted/40", className)}>
      <pre aria-label={label} className="overflow-x-auto p-3 pr-12 font-mono text-xs leading-relaxed"><code>{code}</code></pre>
      <button
        type="button"
        aria-label={state === "copied" ? t("code.copied") : t("code.copy")}
        onClick={() => void copy()}
        className="absolute right-2 top-2 rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        {state === "copied" ? <Check className="size-4" /> : <Copy className="size-4" />}
      </button>
      {state === "failed" && <p role="status" className="px-3 pb-2 text-xs text-muted-foreground">{t("code.copyFailed")}</p>}
    </div>
  );
}

/** Tables styled like the panel's lists. */
export function Table({ className, ...props }: React.ComponentProps<"table">) {
  return (
    <div className="relative w-full min-w-0 max-w-full overflow-x-auto">
      <table {...props} className={cn("w-full text-sm", className)} />
    </div>
  );
}
export function TableHeader({ className, ...props }: React.ComponentProps<"thead">) {
  return <thead {...props} className={cn("border-b border-border bg-muted/40 text-left text-xs text-muted-foreground", className)} />;
}
export function TableBody({ className, ...props }: React.ComponentProps<"tbody">) {
  return <tbody {...props} className={className} />;
}
export function TableRow({ className, ...props }: React.ComponentProps<"tr">) {
  return <tr {...props} className={cn("border-b border-border last:border-0 hover:bg-muted/50", className)} />;
}
export function TableHead({ className, ...props }: React.ComponentProps<"th">) {
  return <th scope="col" {...props} className={cn("whitespace-nowrap p-3 font-medium", className)} />;
}
export function TableCell({ className, ...props }: React.ComponentProps<"td">) {
  return <td {...props} className={cn("p-3", className)} />;
}
