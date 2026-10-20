// The brand: its logo (the dark one when the page shows dark) and name, from
// the index configuration, so the whole document shows one branding until a
// reload (runtime branding may change meanwhile, ADR 0033).
import { useEffect, useSyncExternalStore } from "react";
import { PanelsTopLeft } from "lucide-react";
import { usePanelConfig } from "@/api/context";
import { cn } from "@/lib/utils";

/** The product name, without a brand of the application's own. */
export const PRODUCT = "rustcms";

function subscribe(change: () => void) {
  const observer = new MutationObserver(change);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-scheme"] });
  return () => observer.disconnect();
}

/** The page shows its dark scheme (`data-scheme` on `<html>`). */
export function useDark(): boolean {
  return useSyncExternalStore(subscribe, () => document.documentElement.dataset.scheme === "dark", () => false);
}

/** The brand name: the index configuration's, else `fallback`, else the product's. */
export function useBrand(fallback?: string): string {
  return usePanelConfig().brand ?? fallback ?? PRODUCT;
}

/** The logo, or the default mark. */
export function BrandMark({ size = "sm" }: { size?: "sm" | "md" }) {
  const { basePath, theme } = usePanelConfig();
  const dark = useDark();
  const logo = (dark && theme?.logoDark) || theme?.logo;
  const box = size === "md" ? "size-9" : "size-8";
  if (logo) return <img src={`${basePath}/${logo}`} alt="" className={cn(box, "shrink-0 rounded-lg object-contain")} />;
  return (
    <span className={cn(box, "flex shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground")}>
      <PanelsTopLeft className={size === "md" ? "size-5" : "size-4"} />
    </span>
  );
}

/** The logo and name above the sign-in and recovery forms; also the tab title. */
export function BrandHeader() {
  const brand = useBrand();
  useEffect(() => {
    document.title = brand;
  }, [brand]);
  return (
    <div className="flex items-center gap-2.5 text-base font-semibold tracking-tight">
      <BrandMark size="md" />
      <span className="break-all">{brand}</span>
    </div>
  );
}
