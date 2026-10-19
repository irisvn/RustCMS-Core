import { useEffect, useRef, useState } from "react";
import type * as React from "react";
import { cn } from "@/lib/utils";
import { PortalContainer } from "./portal";

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
      aria-label={title}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      className={cn(
        "m-auto max-h-[calc(100dvh-2rem)] w-[calc(100%-2rem)] max-w-md overflow-y-auto rounded-xl border border-border bg-card p-0 text-foreground shadow-xl backdrop:bg-black/50",
        className,
      )}
    >
      {open && (
        <PortalContainer.Provider value={element}>
          <div className="flex flex-col gap-4 p-5">
            <h2 className="text-base font-semibold">{title}</h2>
            {children}
          </div>
        </PortalContainer.Provider>
      )}
    </dialog>
  );
}
