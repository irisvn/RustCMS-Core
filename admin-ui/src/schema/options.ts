// Options computed on the server (ADR 0044, part E): an input with
// `optionsFrom` shows the options the server answered last for the form's
// state, the declared ones until then (and when asking fails).
import type { ComponentSchema, InputSchema, SelectOption } from "@/api/types";
import { isInput, isLayout } from "@/schema/form";

/** The latest answer of each input asking `form/options`, by name. */
export type ComputedOptions = Record<string, SelectOption[]>;

/** The inputs that ask for their options: those with `optionsFrom` the user may change. */
export function optionInputs(inputs: InputSchema[]): InputSchema[] {
  return inputs.filter((i) => i.optionsFrom && !i.disabled);
}

/** The inputs whose options depend on input `name` (asked again when it changes). */
export function optionDependents(inputs: InputSchema[], name: string): string[] {
  return optionInputs(inputs)
    .filter((i) => i.optionsFrom!.dependsOn.includes(name))
    .map((i) => i.name);
}

/** `components` with each computed answer in place of the declared options. */
export function withOptions(components: ComponentSchema[], computed: ComputedOptions): ComponentSchema[] {
  if (Object.keys(computed).length === 0) return components;
  return components.map((c) => {
    if (isInput(c)) {
      const options = c.optionsFrom ? computed[c.name] : undefined;
      return options ? { ...c, options } : c;
    }
    if (isLayout(c)) return { ...c, components: withOptions(c.components, computed) };
    return c;
  });
}
