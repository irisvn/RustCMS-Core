import { render } from "@testing-library/react";
import { BarChart, formatCount } from "@/components/dashboard";

it("formats counts beyond 2^53 without losing digits", () => {
  expect(formatCount("9007199254740993", "en")).toBe("9,007,199,254,740,993");
  expect(formatCount("1234", "vi")).toBe("1.234");
  expect(formatCount("not a number", "en")).toBe("not a number");
});

it("draws charts with shapes and classes only (no inline style: CSP)", () => {
  const buckets = [
    { start: "2026-10-19", count: "0" },
    { start: "2026-10-20", count: "3" },
    { start: "2026-10-21", count: "6" },
  ];
  const { container } = render(<BarChart buckets={buckets} label="Per day" />);
  expect(container.querySelectorAll("rect")).toHaveLength(3);
  expect(container.querySelector("[style]")).toBeNull();
  const heights = [...container.querySelectorAll("rect")].map((r) => Number(r.getAttribute("height")));
  expect(heights[2]).toBeGreaterThan(heights[1]!);
  expect(heights[0]).toBe(1);
  // Readable without the picture.
  expect(container.querySelector("table.sr-only tbody")?.textContent).toContain("6");
});
