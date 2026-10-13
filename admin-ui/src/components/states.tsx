import { Ban, FileQuestion, Loader2, TriangleAlert } from "lucide-react";
import type * as React from "react";
import { ApiError } from "@/api/client";
import { useUncertain } from "@/api/context";
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

/** Shown while writes of `scope` are paused after a write with an unknown outcome. */
export function PausedBanner({ scope }: { scope: string }) {
  const uncertain = useUncertain(scope);
  if (!uncertain) return null;
  return (
    <Alert variant="warning">
      Actions are paused until you reload the page and check the result.{" "}
      <button type="button" className="underline" onClick={() => window.location.reload()}>
        Reload
      </button>
    </Alert>
  );
}
