// A custom page of the application (`/p/{slug}`).
import { useParams, useRouterState } from "@tanstack/react-router";
import { useCustomPage } from "@/api/custom";
import { CustomContentView } from "@/components/custom";
import { Page, PageHeader } from "@/components/layout";
import { ErrorState, Loading, NotFoundPage } from "@/components/states";
import { usePanelConfig } from "@/api/context";
import { canonicalSubPath } from "@/lib/paths";

export function CustomPage() {
  const { slug } = useParams({ strict: false }) as { slug: string };
  const config = usePanelConfig();
  // The sub-path from the raw (still encoded) address, checked with the
  // server's rules before the router's decoding is trusted.
  const href = useRouterState({ select: (s) => s.location.href });
  // The browser's raw path when it is the panel's (production), else the
  // router's own (still encoded) href.
  const browser = globalThis.location?.pathname ?? "";
  const base = config.basePath.replace(/\/$/, "");
  const pathname = browser.startsWith(`${base}/p/`) ? browser.slice(base.length) : href.split(/[?#]/)[0]!;
  const raw = rawSubPath(slug, pathname);
  const path = raw === null ? "" : canonicalSubPath(raw);
  const page = useCustomPage(slug, path ?? "", { enabled: path !== null });
  if (path === null) return <NotFoundPage />;
  if (page.isLoading) return <Loading />;
  // A failed refetch (no longer allowed, gone) drops what was shown before.
  if (page.isError || !page.data) return <ErrorState error={page.error} />;
  return (
    <Page>
      <PageHeader title={page.data.title} />
      <CustomContentView kind="page" content={page.data.content} reload={() => void page.refetch()}
        place={{ kind: "page", slug, path }} />
    </Page>
  );
}

/** What follows `/p/{slug}/` in `pathname` (panel-relative), or `null` when it is not this page. */
function rawSubPath(slug: string, pathname: string): string | null {
  const prefix = `/p/${encodeURIComponent(slug)}`;
  if (!pathname.startsWith(prefix)) return null;
  const rest = pathname.slice(prefix.length);
  return rest.startsWith("/") ? rest.slice(1) : rest === "" ? "" : null;
}

/** The slots beside a record, each in a card. */
export { RecordSlots } from "@/components/slots";
