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
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const raw = rawSubPath(config.basePath, slug, globalThis.location?.pathname ?? pathname);
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

/** What follows `{basePath}/p/{slug}/` in `pathname`, or `null` when there is none. */
function rawSubPath(basePath: string, slug: string, pathname: string): string | null {
  const prefix = `${basePath.replace(/\/$/, "")}/p/${encodeURIComponent(slug)}`;
  if (!pathname.startsWith(prefix)) return null;
  const rest = pathname.slice(prefix.length);
  return rest.startsWith("/") ? rest.slice(1) : rest === "" ? "" : null;
}

/** The slots beside a record, each in a card. */
export { RecordSlots } from "@/components/slots";
