import { dateOf, dayStart, nextDay } from "@/renderers/table";

describe("date range filter bounds", () => {
  it("ends a day at the start of the next one (any precision)", () => {
    expect(nextDay("2026-10-12")).toBe("2026-10-13");
    expect(nextDay("2026-10-31")).toBe("2026-11-01");
    expect(nextDay("2026-12-31")).toBe("2027-01-01");
    const until = dayStart("local_datetime", nextDay("2026-10-12"));
    expect(until).toBe("2026-10-13T00:00:00");
    // 23:59:59.250 on the 12th is before the bound: included.
    expect("2026-10-12T23:59:59.250" < until).toBe(true);
    expect(dateOf("local_datetime", until, true)).toBe("2026-10-12");
  });

  it("uses the browser's midnight for instants", () => {
    const until = dayStart("instant", nextDay("2026-10-12"));
    expect(new Date(until).getTime()).toBe(new Date("2026-10-13T00:00:00").getTime());
    expect(dateOf("instant", until, true)).toBe("2026-10-12");
    expect(dateOf("date", "2026-10-12")).toBe("2026-10-12");
  });
});
