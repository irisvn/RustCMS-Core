// Rules computed on the server (ADR 0044, part E): after an input others'
// rules depend on changes and typing pauses, the form's values go to
// `form/rules`; the answer is applied only if it is the latest and the form
// has not changed since. Failures keep what the form had (the save checks again).
import { useCallback, useEffect, useMemo, useRef } from "react";
import type { UseFormReturn } from "react-hook-form";
import type { FormRulesRequest, FormRulesResponse, InputSchema } from "./types";
import { useApi } from "./context";
import type { FormValues } from "@/schema/form";
import { REACTION_DELAY_MS, reactionState } from "@/schema/reactions";
import type { RulesState } from "@/schema/rules";

const enc = encodeURIComponent;

export function useFormRules({ slug, recordKey, via, inputs, dependencies, form, paused, askNow, onRules }: {
  slug: string;
  recordKey?: string;
  via?: { parent: string; record: string; relation: string };
  inputs: InputSchema[];
  /** The inputs whose change asks again. */
  dependencies: Set<string>;
  form: UseFormReturn<FormValues>;
  paused: boolean;
  /** Ask once the form opens: the schema's answer was computed without what this form holds (a relation's key, a blind edit). */
  askNow?: boolean;
  onRules: (rules: RulesState) => void;
}): { onUserChange: (name: string) => void } {
  const api = useApi();
  // Changes of the inputs rules depend on: an answer is stale once one changed after it was asked.
  const changes = useRef(0);
  // An answer dropped while saving: asked again when the form is editable again.
  const dropped = useRef(false);
  const seq = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const live = useRef(true);
  const latest = useRef({ paused, onRules, inputs, via });
  latest.current = { paused, onRules, inputs, via };
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
      if (timer.current) clearTimeout(timer.current);
    };
  }, []);
  const asking = useMemo(() => dependencies, [dependencies]);

  const ask = useCallback(async () => {
    const sent = ++seq.current;
    const before = changes.current;
    const epoch = api.epoch;
    const via = recordKey === undefined ? latest.current.via : undefined;
    const request: FormRulesRequest = {
      operation: recordKey === undefined ? "create" : "edit",
      ...(recordKey === undefined ? {} : { key: recordKey }),
      ...(via ? { via: { parent: via.parent, record: via.record, relation: via.relation } } : {}),
      state: reactionState(latest.current.inputs, form.getValues()),
      seq: sent,
    };
    let answer: FormRulesResponse;
    try {
      answer = await api.post<FormRulesResponse>(`/resources/${enc(slug)}/form/rules`, request);
    } catch {
      return; // network, 429, refused: the rules stay as they were
    }
    const { paused, onRules } = latest.current;
    if (!live.current || api.epoch !== epoch || answer.seq !== seq.current || changes.current !== before || answer.failed) return;
    if (paused) {
      dropped.current = true;
      return;
    }
    onRules(answer.rules);
  }, [api, form, recordKey, slug]);

  useEffect(() => {
    if (!paused && dropped.current) {
      dropped.current = false;
      void ask();
    }
  }, [paused, ask]);
  useEffect(() => {
    if (askNow) void ask();
    // Once, when the form opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onUserChange = useCallback(
    (name: string) => {
      if (!asking.has(name)) return;
      changes.current += 1;
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        timer.current = null;
        void ask();
      }, REACTION_DELAY_MS);
    },
    [ask, asking],
  );
  return { onUserChange };
}
