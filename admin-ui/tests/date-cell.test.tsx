// Date columns (vocabulary 21): the column's pattern, else the panel's; without
// any, as before (dates as written, local date-times as `yyyy-MM-dd HH:mm`).
import { render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { ColumnSchema, Envelope } from "@/api/types";
import { setDateFormats } from "@/i18n";
import { Cell } from "@/renderers/table";

afterEach(() => setDateFormats(undefined, undefined));

const column = (valueType: string, extra: Partial<ColumnSchema> = {}) =>
  ({ name: "at", type: "date", label: "At", valueType, ...extra }) as unknown as ColumnSchema;
const show = (c: ColumnSchema, value: string) =>
  render(<Cell column={c} record={{ key: "1", data: { at: value } } as unknown as Envelope} />);

it("shows dates as written without a pattern", () => {
  const one = show(column("date"), "2026-11-18");
  expect(screen.getByText("2026-11-18").tagName).toBe("TIME");
  one.unmount();
  show(column("local_datetime"), "2026-11-18T09:15:30");
  expect(screen.getByText("2026-11-18 09:15")).toBeTruthy();
});

it("uses the column's pattern", () => {
  show(column("local_datetime", { format: "HH:mm:ss dd/MM/yyyy" }), "2026-11-18T09:15:30");
  const time = screen.getByText("09:15:30 18/11/2026");
  expect(time.getAttribute("datetime")).toBe("2026-11-18T09:15:30");
});

it("uses the panel's pattern for date and text columns", () => {
  setDateFormats("dd/MM/yyyy", "HH:mm dd/MM/yyyy");
  const one = show(column("date"), "2026-11-18");
  expect(screen.getByText("18/11/2026")).toBeTruthy();
  one.unmount();
  show(column("local_datetime", { type: "text" }), "2026-11-18T09:15");
  expect(screen.getByText("09:15 18/11/2026")).toBeTruthy();
});
