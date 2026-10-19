import { cloneElement, createContext, useCallback, useContext, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type * as React from "react";
import { createPortal } from "react-dom";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { focusNextFrom, follow, place, usePortalContainer } from "./portal";

const MenuContext = createContext<{ close: () => void } | null>(null);

/**
 * A menu of actions under a button. Arrows, Home and End move; Enter or Space
 * selects; Escape closes and returns to the button; Tab closes and moves on
 * from the button; a click outside closes it.
 */
export function DropdownMenu({
  trigger,
  children,
  align = "start",
  variant = "outline",
  size = "sm",
  className,
  "aria-label": label,
}: {
  trigger: React.ReactNode;
  children: React.ReactNode;
  align?: "start" | "end";
  variant?: React.ComponentProps<typeof Button>["variant"];
  size?: React.ComponentProps<typeof Button>["size"];
  className?: string;
  "aria-label"?: string;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const container = usePortalContainer();
  const id = useId();
  const close = useCallback((refocus = true) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  }, []);
  useLayoutEffect(() => {
    const anchor = button.current;
    const floating = menu.current;
    if (!open || !anchor || !floating) return;
    const stop = follow(() => place(anchor, floating, align));
    items(floating)[0]?.focus();
    return stop;
  }, [open, align]);
  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => {
      const target = e.target as Node;
      if (!menu.current?.contains(target) && !button.current?.contains(target)) close(false);
    };
    document.addEventListener("pointerdown", outside, true);
    return () => document.removeEventListener("pointerdown", outside, true);
  }, [open, close]);
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const list = items(menu.current);
    const current = list.indexOf(document.activeElement as HTMLElement);
    const moves: Record<string, number> = { ArrowDown: current + 1, ArrowUp: current - 1, Home: 0, End: list.length - 1 };
    if (e.key in moves && list.length > 0) {
      e.preventDefault();
      list[(moves[e.key]! + list.length) % list.length]!.focus();
    } else if (e.key === "Escape") {
      // Only the menu closes, not a dialog around it.
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === "Tab") {
      e.preventDefault();
      setOpen(false);
      const from = button.current;
      if (from) focusNextFrom(from, from.closest("dialog") ?? document, e.shiftKey);
    }
  };
  return (
    <>
      <Button
        ref={button}
        type="button"
        variant={variant}
        size={size}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        className={className}
        onClick={() => setOpen((v) => !v)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" && !open) {
            e.preventDefault();
            setOpen(true);
          }
        }}
      >
        {trigger}
      </Button>
      {open &&
        createPortal(
          <MenuContext.Provider value={{ close: () => close() }}>
            <div
              ref={menu}
              id={id}
              role="menu"
              aria-label={label}
              onKeyDown={onKeyDown}
              className="fixed left-0 top-0 z-50 min-w-40 overflow-y-auto rounded-lg border border-border bg-card p-1 text-sm text-foreground shadow-lg"
            >
              {children}
            </div>
          </MenuContext.Provider>,
          container,
        )}
    </>
  );
}

function items(menu: HTMLElement | null): HTMLElement[] {
  return [...(menu?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([aria-disabled="true"])') ?? [])];
}

export function DropdownMenuItem({
  onSelect,
  destructive,
  disabled,
  children,
}: {
  onSelect?: () => void;
  destructive?: boolean;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  const menu = useContext(MenuContext);
  const select = () => {
    if (disabled) return;
    menu?.close();
    onSelect?.();
  };
  return (
    <div
      role="menuitem"
      tabIndex={-1}
      aria-disabled={disabled || undefined}
      onClick={select}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          select();
        }
      }}
      className={cn(
        "flex min-h-8 cursor-default select-none items-center gap-2 rounded-md px-2 py-1.5 outline-none focus:bg-muted",
        destructive && "text-destructive",
        disabled && "opacity-50",
      )}
    >
      {children}
    </div>
  );
}

export function DropdownMenuSeparator() {
  return <div role="separator" className="-mx-1 my-1 h-px bg-border" />;
}

export function DropdownMenuLabel({ children }: { children: React.ReactNode }) {
  return <div role="presentation" className="px-2 py-1.5 text-xs font-medium text-muted-foreground">{children}</div>;
}

const DELAY = 400;

/**
 * A short description shown on hover and on focus (after a delay), kept
 * while the pointer is over it, closed by Escape. `children` is one element
 * that takes `aria-describedby` and pointer and focus handlers.
 */
export function Tooltip({ content, children, align = "center" }: { content: React.ReactNode; children: React.ReactElement<Record<string, unknown>>; align?: "start" | "end" | "center" }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLElement | null>(null);
  const tip = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const container = usePortalContainer();
  const id = useId();
  const show = (target: HTMLElement, delay: number) => {
    anchor.current = target;
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setOpen(true), delay);
  };
  const hide = (delay = 100) => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setOpen(false), delay);
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  useLayoutEffect(() => {
    const target = anchor.current;
    const floating = tip.current;
    if (!open || !target || !floating) return;
    return follow(() => place(target, floating, align));
  }, [open, align]);
  useEffect(() => {
    if (!open) return;
    // Captured first: Escape closes the tooltip only, not a dialog around it.
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open]);
  const props = children.props;
  const chain =
    (name: string, f: (e: React.SyntheticEvent<HTMLElement>) => void) =>
    (e: React.SyntheticEvent<HTMLElement>) => {
      (props[name] as ((e: unknown) => void) | undefined)?.(e);
      f(e);
    };
  return (
    <>
      {cloneElement(children, {
        "aria-describedby": open ? id : (props["aria-describedby"] as string | undefined),
        onPointerEnter: chain("onPointerEnter", (e) => show(e.currentTarget, DELAY)),
        onPointerLeave: chain("onPointerLeave", () => hide()),
        onFocus: chain("onFocus", (e) => show(e.currentTarget, DELAY)),
        onBlur: chain("onBlur", () => hide(0)),
      })}
      {open &&
        createPortal(
          <div
            ref={tip}
            id={id}
            role="tooltip"
            onPointerEnter={() => clearTimeout(timer.current)}
            onPointerLeave={() => hide()}
            className="fixed left-0 top-0 z-50 max-w-xs rounded-md border border-border bg-foreground px-2 py-1 text-xs text-background shadow-md"
          >
            {content}
          </div>,
          container,
        )}
    </>
  );
}
