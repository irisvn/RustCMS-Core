import { useEffect, useId, useRef, useState } from "react";
import type * as React from "react";
import { X } from "lucide-react";
import { useT } from "@/i18n";
import { cn } from "@/lib/utils";
import { PortalContainer } from "./portal";

/**
 * Modal built on the native <dialog> (focus trap, Escape, inert background)
 * rather than a library that injects inline styles the CSP would block.
 * A close button sits after the content (opening still focuses the first
 * field); `closeDisabled` blocks it and Escape while the dialog is busy.
 */
export function Dialog({
  open,
  onClose,
  title,
  children,
  className,
  closeButton = true,
  closeDisabled = false,
}: {
  open: boolean;
  onClose: () => void;
  title: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  closeButton?: boolean;
  closeDisabled?: boolean;
}) {
  const t = useT();
  const titleId = useId();
  const ref = useRef<HTMLDialogElement>(null);
  // Menus and tooltips inside render into the dialog (the top layer).
  const [element, setElement] = useState<HTMLDialogElement | null>(null);
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
      ref={(el) => {
        ref.current = el;
        setElement(el);
      }}
      aria-labelledby={titleId}
      onCancel={(e) => {
        e.preventDefault();
        if (!closeDisabled) onClose();
      }}
      className={cn(
        "m-auto max-h-[calc(100dvh-2rem)] w-[calc(100%-2rem)] max-w-md overflow-y-auto rounded-xl border border-border bg-card p-0 text-foreground shadow-xl backdrop:bg-black/50",
        className,
      )}
    >
      {open && (
        <PortalContainer.Provider value={element}>
          <div className="relative flex flex-col gap-4 p-5">
            <h2 id={titleId} className={cn("text-base font-semibold", closeButton && "pr-8")}>{title}</h2>
            {children}
            {closeButton && (
              <button
                type="button"
                aria-label={t("action.close")}
                disabled={closeDisabled}
                onClick={onClose}
                className="absolute right-3 top-3 inline-flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
              >
                <X className="size-4" aria-hidden="true" />
              </button>
            )}
          </div>
        </PortalContainer.Provider>
      )}
    </dialog>
  );
}
