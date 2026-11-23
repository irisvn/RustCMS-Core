// Options computed on the server (ADR 0044, part E): each input with
// `optionsFrom` asks `form/options` when the form opens, and again once a
// change of one of its dependencies (typed, or patched by a reaction) pauses.
// Only the latest answer for the dependencies' current values is applied; a
// failure brings back the declared options (the save checks the value anyway).
import { useCallback, useEffect, useRef, useState } from "react";
import type { UseFormReturn } from "react-hook-form";
import type { FormOptionsRequest, FormOptionsResponse, InputSchema } from "./types";
import { useApi } from "./context";
import type { FormValues } from "@/schema/form";
import { type ComputedOptions, optionDependents, optionInputs } from "@/schema/options";
import { REACTION_DELAY_MS, reactionState } from "@/schema/reactions";

const enc = encodeURIComponent;

export interface FormOptions {
  /** The latest answers, by input. */
  options: ComputedOptions;
  /** Call on every change of an input (typed or patched). */
  onChange: (name: string) => void;
}

export function useFormOptions({ slug, recordKey, via, inputs, form, paused }: {
  slug: string;
  /** The edited record (absent: a create form). */
  recordKey?: string;
  /** A create form opened through a relation of another record. */
  via?: { parent: string; record: string; relation: string };
  inputs: InputSchema[];
  form: UseFormReturn<FormValues>;
  /** While true (saving, read-only), answers are not applied (asked again after). */
  paused: boolean;
}): FormOptions {
  const api = useApi();
  const [options, setOptions] = useState<ComputedOptions>({});
  // The latest request of each input, its pending timer, answers dropped while paused.
  const seq = useRef<Record<string, number>>({});
  const timers = useRef<Record<string, ReturnType<typeof setTimeout>>>({});
  const dropped = useRef(new Set<string>());
  const live = useRef(true);
  const latest = useRef({ paused, inputs, via });
  latest.current = { paused, inputs, via };
  useEffect(() => {
    live.current = true;
    const pending = timers.current;
    return () => {
      live.current = false;
      for (const timer of Object.values(pending)) clearTimeout(timer);
    };
  }, []);

  // The values `name`'s options depend on, as sent (to tell whether they changed).
  const dependencies = useCallback(
    (name: string) => {
      const input = latest.current.inputs.find((i) => i.name === name);
      const state = reactionState(latest.current.inputs, form.getValues());
      return JSON.stringify((input?.optionsFrom?.dependsOn ?? []).map((d) => state[d] ?? null));
    },
    [form],
  );

  const ask = useCallback(
    async (name: string) => {
      const sent = (seq.current[name] ?? 0) + 1;
      seq.current[name] = sent;
      const before = dependencies(name);
      // An answer from an earlier session (sign-out, another user) is never applied.
      const epoch = api.epoch;
      const via = recordKey === undefined ? latest.current.via : undefined;
      const request: FormOptionsRequest = {
        operation: recordKey === undefined ? "create" : "edit",
        ...(recordKey === undefined ? {} : { key: recordKey }),
        ...(via ? { via: { parent: via.parent, record: via.record, relation: via.relation } } : {}),
        input: name,
        state: reactionState(latest.current.inputs, form.getValues()),
      };
      let answer: FormOptionsResponse | null;
      try {
        answer = await api.post<FormOptionsResponse>(`/resources/${enc(slug)}/form/options`, request);
      } catch {
        answer = null; // network, 429, refused: as a failure
      }
      if (!live.current || api.epoch !== epoch || seq.current[name] !== sent) return;
      // A dependency changed since: a newer request is on its way.
      if (dependencies(name) !== before) return;
      if (latest.current.paused) {
        dropped.current.add(name);
        return;
      }
      const computed = answer && !answer.failed ? answer.options : undefined;
      setOptions((current) => {
        if (computed) return { ...current, [name]: computed };
        if (!(name in current)) return current;
        const rest = { ...current };
        delete rest[name];
        return rest;
      });
    },
    [api, dependencies, form, recordKey, slug],
  );

  // Once, when the form opens.
  useEffect(() => {
    for (const input of optionInputs(inputs)) void ask(input.name);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // Answers dropped while saving: asked again once the form is editable.
  useEffect(() => {
    if (paused || dropped.current.size === 0) return;
    const names = [...dropped.current];
    dropped.current.clear();
    for (const name of names) void ask(name);
  }, [paused, ask]);

  const onChange = useCallback(
    (changed: string) => {
      for (const name of optionDependents(latest.current.inputs, changed)) {
        const timer = timers.current[name];
        if (timer) clearTimeout(timer);
        timers.current[name] = setTimeout(() => {
          delete timers.current[name];
          void ask(name);
        }, REACTION_DELAY_MS);
      }
    },
    [ask],
  );

  return { options, onChange };
}
