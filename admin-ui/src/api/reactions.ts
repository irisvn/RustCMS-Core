// Form reactions (ADR 0043): after an input with `reacts` changes and typing
// pauses, the form's values go to the server's reaction; its answer is
// applied only if it is the latest one and the form has not changed since
// (a newer request is then on its way). Failures are ignored: the form works
// without reactions (they never run on save).
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import type { UseFormReturn } from "react-hook-form";
import { pushToast } from "@/lib/toast";
import type { FormReactRequest, FormReactResponse, InputSchema, LinkPage } from "./types";
import { useApi } from "./context";
import type { FormValues } from "@/schema/form";
import { REACTION_DELAY_MS, type RowFlags, type StoredRows, applyPatches, reactionState } from "@/schema/reactions";
import { stateOf } from "@/schema/relation-rows";

const enc = encodeURIComponent;

export interface FormReactions {
  /** Call on every change the user makes (any input). */
  onUserChange: (name: string) => void;
  /** Warnings under rows, from the latest answer. */
  flags: RowFlags;
}

export function useFormReactions({ slug, recordKey, via, inputs, form, paused, onPatched }: {
  slug: string;
  /** The edited record (absent: a create form). */
  recordKey?: string;
  /** A create form opened through a relation of another record. */
  via?: { parent: string; record: string; relation: string };
  inputs: InputSchema[];
  form: UseFormReturn<FormValues>;
  /** While true (saving, read-only), answers are not applied. */
  paused: boolean;
  /** An input the answer changed (a create form then sends it). */
  onPatched: (name: string) => void;
}): FormReactions {
  const api = useApi();
  const queries = useQueryClient();
  // A relationship repeater's stored rows, as its widget read them (ADR 0044, part D).
  const stored: StoredRows = (input) => {
    const relationship = input.repeater?.relationship;
    if (!relationship) return null;
    if (recordKey === undefined) return { rows: [], deleted: [] };
    const page = queries.getQueryData<LinkPage>(["records", relationship.resource, "relation", slug, recordKey, relationship.relation, "repeater"]);
    // More rows than the widget reads: it is read-only (edited in the relation section), and so for reactions.
    if (!page || page.meta.total > page.data.length) return null;
    return stateOf(input.repeater!, page.data);
  };
  const storedRef = useRef(stored);
  storedRef.current = stored;
  const [flags, setFlags] = useState<RowFlags>({});
  // Changes the user made so far, and the latest request.
  const changes = useRef(0);
  const seq = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const live = useRef(true);
  const latest = useRef({ paused, onPatched, inputs, via });
  latest.current = { paused, onPatched, inputs, via };
  // Each reacting input's value when the form opened or an answer was last applied
  // (`previous`: a slug follows the name while it still is the old name's).
  const baseline = useRef<Record<string, unknown> | null>(null);
  if (baseline.current === null) baseline.current = reactionState(inputs, form.getValues(), stored);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
      if (timer.current) clearTimeout(timer.current);
    };
  }, []);

  const ask = useCallback(
    async (changed: string) => {
      const sent = ++seq.current;
      const before = changes.current;
      // An answer from an earlier session (sign-out, another user) is never applied.
      const epoch = api.epoch;
      const via = recordKey === undefined ? latest.current.via : undefined;
      const previous = baseline.current?.[changed];
      const request: FormReactRequest = {
        operation: recordKey === undefined ? "create" : "edit",
        ...(recordKey === undefined ? {} : { key: recordKey }),
        ...(via ? { via: { parent: via.parent, record: via.record, relation: via.relation } } : {}),
        ...(previous === undefined ? {} : { previous: previous as FormReactRequest["previous"] }),
        changed,
        state: reactionState(latest.current.inputs, form.getValues(), storedRef.current),
        seq: sent,
      };
      let answer: FormReactResponse;
      try {
        answer = await api.post<FormReactResponse>(`/resources/${enc(slug)}/form/react`, request);
      } catch {
        return; // network, 429, refused: no help this time
      }
      const { paused, onPatched, inputs } = latest.current;
      if (!live.current || api.epoch !== epoch || answer.seq !== seq.current || changes.current !== before || paused || answer.failed) return;
      const applied = applyPatches(inputs, form.getValues(), answer.patches, storedRef.current);
      for (const [name, value] of Object.entries(applied.values)) {
        form.setValue(name, value, { shouldDirty: true });
        onPatched(name);
      }
      setFlags(applied.flags);
      baseline.current = reactionState(inputs, form.getValues(), storedRef.current);
      for (const notice of applied.notices) pushToast("success", notice, epoch);
    },
    [api, form, recordKey, slug],
  );

  const onUserChange = useCallback(
    (name: string) => {
      changes.current += 1;
      // Warnings under rows point at rows by place: stale once the user changes them.
      setFlags((flags) => {
        if (!(name in flags)) return flags;
        const rest = { ...flags };
        delete rest[name];
        return rest;
      });
      if (!latest.current.inputs.some((i) => i.name === name && i.reacts && !i.disabled)) return;
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        timer.current = null;
        void ask(name);
      }, REACTION_DELAY_MS);
    },
    [ask],
  );

  return { onUserChange, flags };
}
