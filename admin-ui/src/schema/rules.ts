// Rules computed on the server (ADR 0044, part E): an input's `serverRules`
// say whether it is shown and required, and its options, for the form's
// latest state; applied to the form's components as the server answers.
import type { ComponentSchema, Condition, EvaluatedRules, InputSchema } from "@/api/types";

/** Never holds: what hides an input the server's rules hide, as a condition would. */
const NEVER: Condition = { op: "any", conditions: [] };

/** The latest answer, by input. */
export type RulesState = Record<string, EvaluatedRules>;

const isInput = (c: ComponentSchema): c is InputSchema => "name" in c && "valueType" in c;

/** The rules as the schema states them (its first answer). */
export function initialRules(components: ComponentSchema[]): RulesState {
  const out: RulesState = {};
  const walk = (list: ComponentSchema[]) => {
    for (const c of list) {
      if (isInput(c)) {
        if (c.serverRules) out[c.name] = { shown: c.serverRules.shown ?? true, required: c.serverRules.required ?? false };
      } else if ("components" in c && c.components) {
        walk(c.components);
      }
    }
  };
  walk(components);
  return out;
}

/** The inputs whose change asks the server again. */
export function ruleDependencies(components: ComponentSchema[]): Set<string> {
  const out = new Set<string>();
  const walk = (list: ComponentSchema[]) => {
    for (const c of list) {
      if (isInput(c)) c.serverRules?.dependsOn.forEach((n) => out.add(n));
      else if ("components" in c && c.components) walk(c.components);
    }
  };
  walk(components);
  return out;
}

/**
 * `components` with `rules` applied: an input's `serverRules` hold the latest
 * answer (a hidden one is hidden as by a condition), a required one gains the
 * required rule, computed options replace the declared ones.
 */
export function withRules(components: ComponentSchema[], rules: RulesState): ComponentSchema[] {
  return components.map((c) => {
    if (isInput(c)) {
      const rule = c.serverRules && rules[c.name];
      if (!c.serverRules || !rule) return c;
      const required = c.serverRules.required !== undefined && rule.required;
      const hidden = c.serverRules.shown !== undefined && !rule.shown;
      return {
        ...c,
        shownWhen: hidden ? (c.shownWhen ? { op: "all", conditions: [c.shownWhen, NEVER] } : NEVER) : c.shownWhen,
        serverRules: {
          ...c.serverRules,
          ...(c.serverRules.shown !== undefined ? { shown: rule.shown } : {}),
          ...(c.serverRules.required !== undefined ? { required: rule.required } : {}),
        },
        options: rule.options ?? c.options,
        rules: required && !c.rules.some((r) => r.rule === "required") ? [...c.rules, { rule: "required" }] : c.rules,
      } as InputSchema;
    }
    if ("components" in c && c.components) return { ...c, components: withRules(c.components, rules) } as ComponentSchema;
    return c;
  });
}
