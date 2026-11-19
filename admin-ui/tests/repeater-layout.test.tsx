// Repeaters laid out as a table (ADR 0044, part A): one column per input
// with the labels in a header (each cell keeps its label for screen
// readers), an empty cell for an input its condition hides; one set of
// inputs whatever the width.
import { render, screen, within } from "@testing-library/react";
import { useForm } from "react-hook-form";
import type { ComponentSchema, InputSchema } from "@/api/types";
import { Components } from "@/renderers/form";
import { type FormValues, defaultValues, inputsOf } from "@/schema/form";

const text = (name: string, extra: Partial<InputSchema> = {}) =>
  ({ type: "text_input", name, label: name[0]!.toUpperCase() + name.slice(1), valueType: "string", nullable: true, disabled: false, rules: [], columnSpan: 1, ...extra }) as InputSchema;

const components: ComponentSchema[] = [{
  ...text("options"),
  type: "repeater",
  valueType: "json",
  repeater: {
    layout: "table",
    addable: true, deletable: true, reorderable: false,
    components: [
      text("content", { rules: [{ rule: "required" }] }),
      { ...text("correct"), type: "toggle", valueType: "bool", nullable: false },
      text("why", { shownWhen: { op: "is_true", name: "correct" } }),
    ],
  },
} as InputSchema];

function Harness() {
  const form = useForm<FormValues>({
    defaultValues: defaultValues(inputsOf(components), { options: [{ content: "a", correct: true, why: "w" }, { content: "b", correct: false }] }),
  });
  return <Components components={components} ctx={{ slug: "quizzes", control: form.control, errors: form.formState.errors }} />;
}

it("lays rows out as a table with a header, labels kept for screen readers", () => {
  const { container } = render(<Harness />);
  // The header names the columns, hidden from assistive technology (cells keep their labels).
  const header = container.querySelector('[aria-hidden="true"].md\\:flex')!;
  expect(header.textContent).toBe("Content *CorrectWhy");
  const rows = screen.getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  expect(within(rows[0]!).getByLabelText(/^Content/)).toHaveValue("a");
  expect(within(rows[0]!).getByLabelText(/^Why/)).toHaveValue("w");
  // The second row's hidden input keeps its column: an empty cell.
  expect(within(rows[1]!).queryByLabelText(/^Why/)).toBeNull();
  expect(rows[1]!.querySelectorAll('div[aria-hidden="true"].md\\:block')).toHaveLength(1);
  // Labels are visually hidden from md on, never removed.
  expect(rows[0]!.querySelector("label")!.className).toContain("md:sr-only");
});
