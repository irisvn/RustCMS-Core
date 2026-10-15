// Exporting a list as CSV: at once, or in the background when it is large;
// the user's background exports of the resource, with their files.
import { Download, FileDown } from "lucide-react";
import { useState } from "react";
import { ApiError } from "@/api/client";
import { useDownloadExport, useExportLater, useExportNow, useExports } from "@/api/exports";
import type { ExportView } from "@/api/types";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { errorText, show, type Text, useT } from "@/i18n";

/** Export button of a list; `query` is the list's query string without its page. */
export function ExportButton({ slug, query }: { slug: string; query: URLSearchParams }) {
  const t = useT();
  const now = useExportNow(slug);
  const later = useExportLater(slug);
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: Text; later?: boolean } | null>(null);
  const exportNow = async () => {
    setNotice(null);
    try {
      await now.mutateAsync(query);
    } catch (e) {
      const tooMany = e instanceof ApiError && e.hasIssue("too_many_to_export");
      setNotice({ variant: "destructive", text: tooMany ? () => t("export.tooMany") : errorText(e, "export.failed"), later: tooMany });
    }
  };
  const exportLater = async () => {
    setNotice(null);
    try {
      await later.mutateAsync(query);
      setNotice({ variant: "default", text: () => t("export.started") });
    } catch (e) {
      setNotice({ variant: "destructive", text: errorText(e, "export.failed") });
    }
  };
  return (
    <div className="flex flex-col items-end gap-2">
      <Button variant="outline" disabled={now.isPending || later.isPending} onClick={() => void exportNow()}>
        <FileDown /> {t("export.now")}
      </Button>
      {notice && (
        <Alert variant={notice.variant} className="max-w-sm text-sm">
          {show(notice.text)}
          {notice.later && (
            <Button size="sm" className="mt-2" disabled={later.isPending} onClick={() => void exportLater()}>
              {t("export.later")}
            </Button>
          )}
        </Alert>
      )}
    </div>
  );
}

function status(t: ReturnType<typeof useT>, item: ExportView): string {
  switch (item.status) {
    case "pending": return t("export.pending");
    case "running": return t("export.running");
    case "done": return t("export.done");
    default: return t("export.error");
  }
}

/** The user's background exports of `slug` (nothing when there are none). */
export function ExportsPanel({ slug }: { slug: string }) {
  const t = useT();
  const exports = useExports(slug);
  const download = useDownloadExport();
  const [failure, setFailure] = useState<Text | null>(null);
  const items = exports.data ?? [];
  if (items.length === 0) return null;
  const get = async (item: ExportView) => {
    setFailure(null);
    try {
      await download.mutateAsync(item);
    } catch (e) {
      setFailure(e instanceof ApiError && e.code === "export_stale" ? () => t("export.stale") : errorText(e, "export.failed"));
    }
  };
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("export.title")}</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {failure && <Alert variant="destructive">{show(failure)}</Alert>}
        <ul className="divide-y divide-border text-sm">
          {items.map((item) => (
            <li key={item.id} className="flex flex-wrap items-center justify-between gap-2 py-2">
              <span className="flex flex-wrap items-center gap-2">
                <Badge color={item.status === "done" ? "success" : item.status === "failed" ? "danger" : "gray"}>{status(t, item)}</Badge>
                <span className="tabular-nums text-muted-foreground">{new Date(item.createdAt).toLocaleString()}</span>
                {item.status === "done" && <span>{t("export.rows", { count: item.rows })}</span>}
                {item.error && <span className="text-destructive">{item.error}</span>}
              </span>
              {item.status === "done" && (
                <Button size="sm" variant="outline" disabled={download.isPending} onClick={() => void get(item)}>
                  <Download /> {t("export.download")}
                </Button>
              )}
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}
