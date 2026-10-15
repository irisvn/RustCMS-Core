// A custom page of the application (`/p/{slug}`).
import { useParams } from "@tanstack/react-router";
import { useCustomPage } from "@/api/custom";
import { CustomContentView } from "@/components/custom";
import { Page, PageHeader } from "@/components/layout";
import { ErrorState, Loading } from "@/components/states";

export function CustomPage() {
  const { slug } = useParams({ strict: false }) as { slug: string };
  const page = useCustomPage(slug);
  if (page.isLoading) return <Loading />;
  if (!page.data) return <ErrorState error={page.error} />;
  return (
    <Page>
      <PageHeader title={page.data.title} />
      <CustomContentView kind="page" content={page.data.content} reload={() => void page.refetch()} />
    </Page>
  );
}

/** The slots beside a record, each in a card. */
export { RecordSlots } from "@/components/slots";
