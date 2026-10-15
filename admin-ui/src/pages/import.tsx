// Importing a CSV file into a resource: upload, match columns with fields,
// check every row, then import (a job writes the rows; this page follows it).
import { Link } from "@tanstack/react-router";
import { Upload } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { useImport, usePreviewImport, useRunImport, useUploadImport } from "@/api/imports";
import { useSchema } from "@/api/queries";
import type { ImportPreview, ImportView, RowIssue } from "@/api/types";
import { Page, PageHeader } from "@/components/layout";
import { ErrorState, Loading } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Button, buttonVariants } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { NativeSelect } from "@/components/ui/input";
import { errorText, issueText, show, type Text, useT } from "@/i18n";

function reason(t: ReturnType<typeof useT>, code: string): string {
  switch (code) {
    case "revoked": return t("import.reason.revoked");
    case "account_changed": return t("import.reason.account_changed");
    case "unfinished": return t("import.reason.unfinished");
    default: return t("import.reason.failed");
  }
}

/** Problems of rows, translated (never values of the file). */
function Report({ issues, labels }: { issues: RowIssue[]; labels: Record<string, string> }) {
  const t = useT();
  if (issues.length === 0) return null;
  return (
    <div className="max-w-full overflow-x-auto">
      <table className="w-full text-sm">
        <thead className="border-b border-border text-left text-xs text-muted-foreground">
          <tr>
            <th scope="col" className="p-2 font-medium">{t("import.row")}</th>
            <th scope="col" className="p-2 font-medium">{t("import.field")}</th>
            <th scope="col" className="p-2 font-medium">{t("import.problem")}</th>
          </tr>
        </thead>
        <tbody>
          {issues.map((issue, i) => (
            <tr key={`${issue.row}-${i}`} className="border-b border-border last:border-0">
              <td className="p-2 tabular-nums">{issue.row}</td>
              <td className="p-2">{issue.field ? (labels[issue.field] ?? issue.field) : "—"}</td>
              <td className="p-2">{issueText({ code: issue.code, params: issue.params }, issue.message)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {issues.length >= 1000 && <p className="p-2 text-xs text-muted-foreground">{t("import.more")}</p>}
    </div>
  );
}

export function ImportPage({ slug }: { slug: string }) {
  const t = useT();
  const schema = useSchema(slug, "view");
  const upload = useUploadImport(slug);
  const previewImport = usePreviewImport();
  const runImport = useRunImport();
  const [uploaded, setUploaded] = useState<ImportView | null>(null);
  const [mapping, setMapping] = useState<Record<string, string>>({});
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [started, setStarted] = useState(false);
  const [failure, setFailure] = useState<Text | null>(null);
  const current = useImport(started && uploaded ? uploaded.id : null);
  const fileId = useId();
  useEffect(() => setPreview(null), [mapping]);
  if (schema.isLoading) return <Loading />;
  if (!schema.data) return <ErrorState error={schema.error} />;
  const resource = schema.data.resource;
  const labels = Object.fromEntries((uploaded?.fields ?? []).map((f) => [f.name, f.label]));
  const state = current.data ?? uploaded;

  const choose = async (file: File | undefined) => {
    if (!file) return;
    setFailure(null);
    setPreview(null);
    try {
      const view = await upload.mutateAsync(file);
      setUploaded(view);
      setMapping(view.mapping);
    } catch (e) {
      setFailure(errorText(e, "import.failed"));
    }
  };
  const check = async () => {
    if (!uploaded) return;
    setFailure(null);
    try {
      setPreview(await previewImport.mutateAsync({ id: uploaded.id, mapping }));
    } catch (e) {
      setFailure(errorText(e, "import.failed"));
    }
  };
  const start = async () => {
    if (!uploaded) return;
    setFailure(null);
    try {
      setUploaded(await runImport.mutateAsync({ id: uploaded.id, mapping }));
      setStarted(true);
    } catch (e) {
      setFailure(errorText(e, "import.failed"));
    }
  };
  const done = (state?.created ?? 0) + (state?.updated ?? 0) + (state?.failed ?? 0);

  return (
    <Page className="max-w-4xl">
      <PageHeader
        title={t("import.title", { resource: resource.pluralLabel })}
        description={t("import.description")}
        actions={<Link to="/r/$slug" params={{ slug }} className={buttonVariants({ variant: "outline" })}>{t("import.back")}</Link>}
      />
      {failure && <Alert variant="destructive">{show(failure)}</Alert>}

      {!started && (
        <Card>
          <CardContent className="flex flex-wrap items-center gap-3 pt-5">
            <label htmlFor={fileId} className="text-sm font-medium">{t("import.file")}</label>
            <input
              id={fileId}
              type="file"
              accept=".csv,text/csv"
              className="text-sm"
              disabled={upload.isPending}
              onChange={(e) => void choose(e.target.files?.[0])}
            />
            {upload.isPending && <span className="text-sm text-muted-foreground"><Upload className="inline size-4" /> {t("import.uploading")}</span>}
            {uploaded && <span className="text-sm text-muted-foreground">{t("import.rows", { count: uploaded.rows })}</span>}
          </CardContent>
        </Card>
      )}

      {uploaded && !started && (
        <Card>
          <CardContent className="flex flex-col gap-4 pt-5">
            <table className="w-full text-sm">
              <thead className="border-b border-border text-left text-xs text-muted-foreground">
                <tr>
                  <th scope="col" className="p-2 font-medium">{t("import.column")}</th>
                  <th scope="col" className="p-2 font-medium">{t("import.field")}</th>
                </tr>
              </thead>
              <tbody>
                {uploaded.header.map((column) => (
                  <tr key={column} className="border-b border-border last:border-0">
                    <td className="p-2 font-mono text-xs">{column}</td>
                    <td className="p-2">
                      <NativeSelect
                        aria-label={column}
                        value={mapping[column] ?? ""}
                        onChange={(e) => {
                          const next = { ...mapping };
                          if (e.target.value) next[column] = e.target.value;
                          else delete next[column];
                          setMapping(next);
                        }}
                      >
                        <option value="">{t("import.none")}</option>
                        {uploaded.fields.map((f) => (
                          <option key={f.name} value={f.name}>
                            {f.key ? `${f.label} (${t("import.key")})` : f.label}
                          </option>
                        ))}
                      </NativeSelect>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" disabled={previewImport.isPending || Object.keys(mapping).length === 0} onClick={() => void check()}>
                {t("import.preview")}
              </Button>
              <Button disabled={runImport.isPending || !preview || preview.valid === 0} onClick={() => void start()}>
                {t("import.run")}
              </Button>
            </div>
            {preview && (
              <div className="flex flex-col gap-2">
                <Alert variant={preview.valid === preview.rows ? "default" : "warning"}>
                  {t("import.valid", { valid: preview.valid, rows: preview.rows })}
                  {preview.keyed > 0 && <> {t("import.keyed", { count: preview.keyed })}</>}
                </Alert>
                <Report issues={preview.report} labels={labels} />
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {started && state && (
        <Card>
          <CardHeader>
            <CardTitle>
              {state.status === "running"
                ? t("import.running", { done, rows: state.rows })
                : state.status === "failed"
                  ? t("import.failed")
                  : t("import.done", { created: state.created, updated: state.updated, failed: state.failed })}
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-2">
            {state.error && <Alert variant="destructive">{reason(t, state.error)}</Alert>}
            <Report issues={state.report} labels={labels} />
          </CardContent>
        </Card>
      )}
    </Page>
  );
}
