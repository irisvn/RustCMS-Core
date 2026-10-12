import { standardSchemaResolver } from "@hookform/resolvers/standard-schema";
import type { FieldErrors, Resolver } from "react-hook-form";
import type { InputSchema } from "@/api/types";
import { type FormValues, zodSchema } from "./form";

/**
 * Client rules as a resolver. On edit only changed fields are submitted, so only
 * they are checked: an untouched field (blank when the user may not view the
 * record, or a legacy value) never blocks saving the others.
 */
export function editResolver(inputs: InputSchema[], editing: boolean, isDirty: (name: string) => boolean): Resolver<FormValues> {
  const base = standardSchemaResolver(zodSchema(inputs)) as Resolver<FormValues>;
  return async (values, context, options) => {
    const result = await base(values, context, options);
    if (!editing || !result.errors) return result;
    const errors = Object.fromEntries(Object.entries(result.errors).filter(([name]) => isDirty(name)));
    return Object.keys(errors).length ? { values: {}, errors: errors as FieldErrors<FormValues> } : { values, errors: {} };
  };
}
