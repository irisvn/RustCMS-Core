import { Ban, FileQuestion, Loader2, TriangleAlert } from "lucide-react";
import type * as React from "react";
import { ApiError } from "@/api/client";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useRetryable, useSession, useUncertain } from "@/api/context";
import { retryPending } from "@/api/queries";
import type { PendingWrite } from "@/app/session";
import { Button } from "@/components/ui/button";
import { Alert } from "@/components/ui/alert";
import { SCHEMA_VERSION, VOCABULARY_VERSION, type ResourceSchema } from "@/api/types";

export function Loading({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="flex items-center gap-2 p-8 text-sm text-muted-foreground" role="status">
      <Loader2 className="size-4 animate-spin" /> {label}
    </div>
  );
}

function Centered({ icon, title, children }: { icon: React.ReactNode; title: string; children?: React.ReactNode }) {
  return (
    <div className="flex flex-col items-center gap-2 p-12 text-center">
      {icon}
      <h1 className="text-lg font-semibold">{title}</h1>
      {children && <p className="text-sm text-muted-foreground">{children}</p>}
    </div>
  );
}

/** 403 and 404 are told apart: records outside the user's scope are 404 by design. */
export function ErrorState({ error }: { error: unknown }) {
  if (error instanceof ApiError && error.status === 403) {
    return <Centered icon={<Ban className="size-8 text-destructive" />} title="Access denied">You are not allowed to see this.</Centered>;
  }
  if (error instanceof ApiError && error.status === 404) {
    return <Centered icon={<FileQuestion className="size-8 text-muted-foreground" />} title="Not found">It does not exist or is not available to you.</Centered>;
  }
  const message = error instanceof Error ? error.message : "Something went wrong.";
  return <Centered icon={<TriangleAlert className="size-8 text-warning" />} title="Something went wrong">{message}</Centered>;
}

export function NotFoundPage() {
  return <Centered icon={<FileQuestion className="size-8 text-muted-foreground" />} title="Page not found" />;
}

/**
 * Compatibility of a schema with this bundle: a newer structure is shown read-only
 * (it could be misread on submit); a newer vocabulary only renders fallbacks.
 */
export function compatibility(schema: ResourceSchema) {
  return {
    readOnly: schema.schemaVersion > SCHEMA_VERSION,
    outdated: schema.schemaVersion > SCHEMA_VERSION || schema.vocabularyVersion > VOCABULARY_VERSION,
  };
}

export function VersionBanner({ schema }: { schema: ResourceSchema }) {
  const { readOnly, outdated } = compatibility(schema);
  if (!outdated) return null;
  return (
    <Alert variant="warning">
      The admin bundle is older than the server; update it to use every feature.
      {readOnly && " Editing is disabled until then."}
    </Alert>
  );
}

/**
 * Shown while writes of `scope` are paused after a write with an unknown
 * outcome. A keyed write (create, action) can be re-sent as is: the server
 * applies it at most once. `onRecovered` receives the server's answer.
 */
export function PausedBanner({ scope, onRecovered }: {
  scope: string;
  /** Called with the answer and the write; returns whether it was handled (e.g. navigated). */
  onRecovered?: (result: unknown, pending: PendingWrite) => Promise<boolean> | boolean;
}) {
  const session = useSession();
  const client = useQueryClient();
  const uncertain = useUncertain(scope);
  const retryable = useRetryable(scope);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  if (!uncertain) {
    return done ? <Alert>The change was applied. The page shows the current data.</Alert> : null;
  }
  const retry = async () => {
    setBusy(true);
    setFailure(null);
    try {
      const { result, pending } = await retryPending(session, scope);
      // Everything settles before the pause lifts: no submission meanwhile.
      try {
        await client.invalidateQueries();
        const handled = (await onRecovered?.(result, pending)) ?? false;
        if (!handled) setDone(true);
      } finally {
        session.resolved(scope, pending.id);
      }
    } catch (e) {
      setFailure(e instanceof Error ? e.message : "The retry failed.");
    } finally {
      setBusy(false);
    }
  };
  return (
    <Alert variant="warning">
      <p>
        {retryable
          ? "The last change may not have been saved. Retrying is safe: it is applied at most once."
          : "Actions are paused until you reload the page and check the result."}
      </p>
      {failure && <p className="mt-1">{failure}</p>}
      <div className="mt-2 flex flex-wrap gap-2">
        {retryable && (
          <Button size="sm" disabled={busy} onClick={() => void retry()}>
            {busy ? "Retrying…" : "Retry safely"}
          </Button>
        )}
        <Button size="sm" variant="outline" onClick={() => window.location.reload()}>
          Reload
        </Button>
      </div>
    </Alert>
  );
}
