// Badge columns draw the color `BadgeColumn::color` gives each value.
import { render, screen } from "@testing-library/react";
import type { ColumnSchema, Envelope } from "@/api/types";
import { Cell } from "@/renderers/table";

const column = {
  name: "status",
  type: "badge",
  label: "Status",
  colors: [
    { value: "draft", color: "gray" },
    { value: "review", color: "warning" },
    { value: "published", color: "success" },
    { value: "featured", color: "primary" },
    { value: "removed", color: "danger" },
    { value: "notice", color: "info" },
    { value: "odd", color: "neon" },
  ],
} as unknown as ColumnSchema;

const cell = (status: string) =>
  render(<Cell column={column} record={{ key: "1", data: { status } } as unknown as Envelope} />);

// Soft pairs from the color tokens; primary is solid (the default primary is
// neutral, a soft one would read as gray).
it("draws each of the six colors, and gray for an unknown one", () => {
  const classes: Record<string, string> = {
    draft: "bg-gray-soft text-gray-soft-foreground",
    review: "bg-warning-soft text-warning-soft-foreground",
    published: "bg-success-soft text-success-soft-foreground",
    featured: "bg-primary text-primary-foreground",
    removed: "bg-destructive-soft text-destructive-soft-foreground",
    notice: "bg-info-soft text-info-soft-foreground",
    odd: "bg-gray-soft",
    unlisted: "bg-gray-soft",
  };
  for (const [status, cls] of Object.entries(classes)) {
    const view = cell(status);
    expect(screen.getByText(status).className, status).toContain(cls);
    view.unmount();
  }
});

// Option labels (vocabulary 20): the column's options name the values; colors
// still follow the values; a value without a label is shown as it is.
const labelled = {
  name: "status",
  type: "badge",
  label: "Status",
  valueType: "string",
  options: [
    { value: "draft", label: "Draft copy" },
    { value: "published", label: "Live" },
  ],
  colors: [{ value: "published", color: "success" }],
} as unknown as ColumnSchema;

const show = (column: ColumnSchema, value: unknown) =>
  render(<Cell column={column} record={{ key: "1", data: { [column.name]: value } } as unknown as Envelope} />);

it("shows the label of a value's option, colored by the value", () => {
  const view = show(labelled, "published");
  expect(screen.getByText("Live").className).toContain("text-success");
  expect(screen.queryByText("published")).toBeNull();
  view.unmount();
  show(labelled, "archived");
  expect(screen.getByText("archived").className).toContain("bg-gray-soft");
});

it("draws one badge per value of a list", () => {
  const { container } = show({ ...labelled, valueType: "json" } as ColumnSchema, ["draft", "published", "odd"]);
  expect([...container.querySelectorAll("span > span")].map((b) => b.textContent)).toEqual(["Draft copy", "Live", "odd"]);
  expect(screen.getByText("Live").className).toContain("text-success");
});

it("shows labels in text columns, a list's joined", () => {
  const text = { ...labelled, type: "text" } as ColumnSchema;
  const one = show(text, "draft");
  expect(screen.getByText("Draft copy")).toBeTruthy();
  one.unmount();
  show({ ...text, valueType: "json" } as ColumnSchema, ["published", "draft"]);
  expect(screen.getByText("Live, Draft copy")).toBeTruthy();
});

it("shows values as they are without options", () => {
  show({ ...labelled, type: "text", options: undefined } as ColumnSchema, "draft");
  expect(screen.getByText("draft")).toBeTruthy();
});
