import { useEffect, useRef } from "react";
import type * as React from "react";
import { cn } from "@/lib/utils";

/**
 * Modal built on the native <dialog> (focus trap, Escape, inert background)
 * rather than a library that injects inline styles the CSP would block.
 */
export function Dialog({
  open,
  onClose,
  title,
  children,
  className,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: React.ReactNode;
  className?: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      if (typeof dialog.showModal === "function") dialog.showModal();
      else dialog.setAttribute("open", "");
    } else if (!open && dialog.open) {
      if (typeof dialog.close === "function") dialog.close();
      else dialog.removeAttribute("open");
    }
  }, [open]);
  return (
    <dialog
      ref={ref}
      aria-label={title}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      className={cn(
        "m-auto w-full max-w-md rounded-lg border border-border bg-card p-0 text-foreground shadow-lg backdrop:bg-black/40",
        className,
      )}
    >
      {open && (
        <div className="flex flex-col gap-4 p-5">
          <h2 className="text-base font-semibold">{title}</h2>
          {children}
        </div>
      )}
    </dialog>
  );
}
