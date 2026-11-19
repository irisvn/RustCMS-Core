// Rules computed on the server (ADR 0044, part E): the schema's first answer,
// the inputs whose change asks again, and the latest answer applied to the
// form (hidden as by a condition, required, computed options).
import type { ComponentSchema, InputSchema } from "@/api/types";
import { defaultValues, hiddenNames, inputsOf, payloadOf } from "@/schema/form";
import { initialRules, ruleDependencies, withRules } from "@/schema/rules";

const text = (name: string, extra: Partial<InputSchema> = {}) =>
  ({ type: "text_input", name, label: name, valueType: "string", nullable: true, disabled: false, rules: [], columnSpan: 1, ...extra }) as InputSchema;

const components: ComponentSchema[] = [
  text("country"),
  text("tax_id", { serverRules: { dependsOn: ["country"], required: false } }),
  { type: "section", label: "More", columns: 1, components: [text("note", { serverRules: { dependsOn: ["country"], shown: true } })] } as unknown as ComponentSchema,
  { ...text("topic", { serverRules: { dependsOn: ["subject"] } }), type: "select", options: [{ value: "poetry", label: "Poetry" }] } as InputSchema,
];

it("reads the schema's answer and what asks again", () => {
  expect(initialRules(components)).toEqual({
    tax_id: { shown: true, required: false },
    note: { shown: true, required: false },
    topic: { shown: true, required: false },
  });
  expect([...ruleDependencies(components)].sort()).toEqual(["country", "subject"]);
});

it("applies the latest answer: hidden as by a condition, required, computed options", () => {
  const ruled = withRules(components, {
    tax_id: { shown: true, required: true },
    note: { shown: false, required: false },
    topic: { shown: true, required: false, options: [{ value: "algebra", label: "Algebra" }] },
  });
  const inputs = inputsOf(ruled);
  expect(inputs.find((i) => i.name === "tax_id")!.rules).toEqual([{ rule: "required" }]);
  expect(inputs.find((i) => i.name === "topic")!.options).toEqual([{ value: "algebra", label: "Algebra" }]);
  // Hidden: not sent, as an input its condition hides.
  const values = { ...defaultValues(inputs, {}), note: "dropped" };
  expect(hiddenNames(ruled, values).has("note")).toBe(true);
  expect(payloadOf(inputs, values, undefined, hiddenNames(ruled, values))).not.toHaveProperty("note");
  // Shown again: no extra condition left.
  const shown = withRules(components, { note: { shown: true, required: false } });
  expect(hiddenNames(shown, values).has("note")).toBe(false);
});
