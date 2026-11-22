// A select whose stored value is no longer among its options (a media
// collection no longer declared) shows that value, not a blank choice.
import { render, screen } from "@testing-library/react";
import { useForm } from "react-hook-form";
import type { InputSchema } from "@/api/types";
import { Components } from "@/renderers/form";
import { type FormValues, defaultValues } from "@/schema/form";

const select: InputSchema = {
  name: "collection", label: "Collection", type: "select", valueType: "string", nullable: true,
  disabled: false, rules: [], columnSpan: 1, placeholder: "Unassigned",
  options: [{ value: "covers", label: "Covers" }],
};

function Harness({ data }: { data: Record<string, unknown> }) {
  const form = useForm<FormValues>({ defaultValues: defaultValues([select], data) });
  return <Components components={[select]} ctx={{ slug: "media", control: form.control, errors: {} }} />;
}

it("shows a stored value that is not an option as it is", () => {
  render(<Harness data={{ collection: "retired" }} />);
  const field = screen.getByLabelText(/^Collection/) as HTMLSelectElement;
  expect(field.selectedOptions[0]?.textContent).toBe("retired");
  expect(field.selectedOptions[0]).toBeDisabled();
});

it("adds nothing for a value among the options, or none", () => {
  const { unmount } = render(<Harness data={{ collection: "covers" }} />);
  let field = screen.getByLabelText(/^Collection/) as HTMLSelectElement;
  expect(field.selectedOptions[0]?.textContent).toBe("Covers");
  expect(field.options).toHaveLength(2);
  unmount();
  render(<Harness data={{ collection: null }} />);
  field = screen.getByLabelText(/^Collection/) as HTMLSelectElement;
  expect(field.selectedOptions[0]?.textContent).toBe("Unassigned");
  expect(field.options).toHaveLength(2);
});
