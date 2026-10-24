import type * as React from "react";
import { cn } from "@/lib/utils";

export function Page({ className, ...props }: React.ComponentProps<"div">) {
  return <div className={cn("mx-auto flex w-full min-w-0 max-w-7xl flex-col gap-6", className)} {...props} />;
}

export function PageHeader({ title, description, actions, eyebrow }: {
  title: string;
  description?: string;
  actions?: React.ReactNode;
  eyebrow?: string;
}) {
  return (
    <div className="flex min-w-0 flex-wrap items-start justify-between gap-4">
      {/* At least 16rem for the title: the actions wrap below it on narrow screens. */}
      <div className="min-w-0 flex-[1_1_16rem] space-y-1.5">
        {eyebrow && <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">{eyebrow}</p>}
        <h1 className="break-words text-2xl font-semibold tracking-tight">{title}</h1>
        {description && <p className="max-w-2xl text-sm leading-relaxed text-muted-foreground">{description}</p>}
      </div>
      {actions && <div className="flex max-w-full flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

export function FormSection({ title, description, children, className }: {
  title: string;
  description?: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("rounded-xl border border-border bg-card p-5 shadow-xs sm:p-6", className)}>
      <div className="mb-5 space-y-1">
        <h2 className="text-base font-semibold tracking-tight">{title}</h2>
        {description && <p className="text-sm leading-relaxed text-muted-foreground">{description}</p>}
      </div>
      {children}
    </section>
  );
}
