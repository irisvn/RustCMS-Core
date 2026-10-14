// Helpers shared by the user and role pages.
import { useEffect, useRef } from "react";
import { ApiError } from "@/api/client";
import { buttonVariants } from "@/components/ui/button";
import { Label } from "@/components/ui/input";

export type Errors = Record<string, string[]>;

/** Field errors of a 422, otherwise one form-level message. */
export function failure(e: unknown): { fields: Errors; form: string | null; stale: boolean } {
  if (e instanceof ApiError && e.status === 422) {
    const form = e.formErrors.join(" ");
    return { fields: e.fieldErrors, form: form || null, stale: form.includes("changed by someone else") };
  }
  if (e instanceof ApiError && e.status === 403) {
    return { fields: {}, form: "You are not allowed to do this.", stale: false };
  }
  return { fields: {}, form: e instanceof Error ? e.message : "Something went wrong.", stale: false };
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
