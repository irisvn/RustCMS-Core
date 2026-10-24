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
    { value: "odd", color: "neon" },
  ],
} as unknown as ColumnSchema;

const cell = (status: string) =>
  render(<Cell column={column} record={{ key: "1", data: { status } } as unknown as Envelope} />);

it("draws each of the five colors, and gray for an unknown one", () => {
  const classes: Record<string, string> = {
    draft: "bg-muted",
    review: "text-warning",
    published: "text-success",
    featured: "text-primary",
    removed: "text-destructive",
    odd: "bg-muted",
    unlisted: "bg-muted",
  };
  for (const [status, cls] of Object.entries(classes)) {
    const view = cell(status);
    expect(screen.getByText(status).className, status).toContain(cls);
    view.unmount();
  }
});
