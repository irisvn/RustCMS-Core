// Helpers shared by the user and role pages.
import { useEffect, useRef } from "react";
import { ApiError } from "@/api/client";
import { buttonVariants } from "@/components/ui/button";
import { Label } from "@/components/ui/input";
import { t } from "@/i18n";

export type Errors = Record<string, string[]>;

export interface Failure {
  /** Field messages (read in the current language). */
  readonly fields: Errors;
  /** A form-level message, if any (read in the current language). */
  readonly form: string | null;
  /** Someone else changed the data meanwhile (by its code, whatever the language). */
  readonly stale: boolean;
}

/** Field errors of a 422, otherwise one form-level message; texts are read when shown. */
export function failure(e: unknown): Failure {
  if (e instanceof ApiError && e.status === 422) {
    return {
      get fields() {
        return e.fieldErrors;
      },
      get form() {
        return e.formErrors.join(" ") || null;
      },
      stale: e.hasIssue("stale"),
    };
  }
  return {
    fields: {},
    get form() {
      return e instanceof ApiError && e.status === 403 ? t("error.forbidden") : e instanceof Error ? e.message : t("state.errorText");
    },
    stale: false,
  };
}

/** Whether the component is still mounted (completion handlers check it). */
export function useMounted() {
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  return mounted;
}

export function Field({ id, label, errors, children }: {
  id: string;
  label: string;
  errors?: string[];
  children: React.ReactNode
}) {
  return (
    <div className="flex min-w-0 flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {errors && <p id={`${id}-error`} role="alert" className="text-xs text-destructive">{errors.join(" ")}</p>}
    </div>
  );
}

export const linkButton = buttonVariants();

// ---------------------------------------------------------------------------
// Users
