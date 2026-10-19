import { X } from "lucide-react";
import { useCallback, useEffect, useSyncExternalStore } from "react";
import { useSession } from "@/api/context";
import { useT } from "@/i18n";
import { type Toast, clearToasts, currentToasts, dismissToast, subscribeToasts } from "@/lib/toast";
import { cn } from "@/lib/utils";

/** How long a toast stays: errors longer. */
const LIFETIME = { success: 5_000, error: 8_000 } as const;

/**
 * The panel's toast region: live regions with no role of their own (they are
 * always there, empty most of the time); each toast is a `status` (polite) or
 * an `alert` (assertive).
 */
export function ToastRegion() {
  const { api } = useSession();
  const subscribe = useCallback((listener: () => void) => api.onEpochChange(listener), [api]);
  const epoch = useSyncExternalStore(subscribe, () => api.epoch);
  useEffect(() => clearToasts(epoch), [epoch]);
  const toasts = useSyncExternalStore(subscribeToasts, currentToasts).filter((t) => t.epoch === epoch);
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-[min(24rem,calc(100%-2rem))] flex-col gap-2">
      <div aria-live="polite" className="flex flex-col gap-2">
        {toasts.filter((t) => t.kind === "success").map((t) => <ToastItem key={t.id} toast={t} />)}
      </div>
      <div aria-live="assertive" className="flex flex-col gap-2">
        {toasts.filter((t) => t.kind === "error").map((t) => <ToastItem key={t.id} toast={t} />)}
      </div>
    </div>
  );
}

function ToastItem({ toast }: { toast: Toast }) {
  const t = useT();
  useEffect(() => {
    const timer = setTimeout(() => dismissToast(toast.id), LIFETIME[toast.kind]);
    return () => clearTimeout(timer);
  }, [toast]);
  return (
    <div
      role={toast.kind === "error" ? "alert" : "status"}
      className={cn(
        "pointer-events-auto flex items-start gap-3 rounded-lg border bg-card p-3 text-sm text-foreground shadow-lg",
        toast.kind === "error" ? "border-destructive" : "border-border",
      )}
    >
      <p className="min-w-0 flex-1 break-words">{toast.text}</p>
      <button
        type="button"
        aria-label={t("toast.close")}
        className="shrink-0 rounded-sm text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        onClick={() => dismissToast(toast.id)}
      >
        <X className="size-4" />
      </button>
    </div>
  );
}
