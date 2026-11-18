// Date patterns (`date_format`): the grammar the server checks, written in the
// panel's language and the browser's (or a given) time zone.
import { afterEach, describe, expect, it } from "vitest";
import { formatDateTime, formatWireDate, hasDateFormat, setDateFormats, setLocale } from "@/i18n";
import { formatPattern, parsePattern } from "@/i18n/dates";

afterEach(() => {
  setLocale("en");
  setDateFormats(undefined, undefined);
});

const at = new Date(Date.UTC(2026, 10, 18, 14, 5, 7)); // Wednesday 18 November 2026, 14:05:07 UTC
const fmt = (pattern: string, locale = "en", timeZone = "UTC") => formatPattern(at, parsePattern(pattern)!, { locale, timeZone });

describe("patterns", () => {
  it("writes every field", () => {
    expect(fmt("yyyy yy MM M dd d HH H hh h mm ss")).toBe("2026 26 11 11 18 18 14 14 02 2 05 07");
    expect(fmt("EEEE EEE, d MMMM (MMM) h a")).toBe("Wednesday Wed, 18 November (Nov) 2 PM");
    expect(fmt("HH:mm z")).toBe("14:05 UTC");
  });

  it("names months, weekdays and AM/PM in the panel's language", () => {
    const vi = fmt("EEEE d MMMM yyyy, h a", "vi");
    expect(vi).toBe("Thứ Tư 18 Tháng 11 2026, 2 CH");
    expect(vi).not.toContain("November");
  });

  it("keeps quoted text and quotes", () => {
    expect(fmt("'at' HH:mm, dd/MM/yyyy")).toBe("at 14:05, 18/11/2026");
    expect(fmt("HH'h'mm 'it''s' dd''MM")).toBe("14h05 it's 18'11");
    expect(fmt("'  'HH:mm' '")).toBe("  14:05 ");
  });

  it("shows instants in the time zone given", () => {
    expect(fmt("HH:mm dd/MM", "en", "Asia/Ho_Chi_Minh")).toBe("21:05 18/11");
    expect(fmt("HH:mm dd/MM", "en", "America/Los_Angeles")).toBe("06:05 18/11");
    // Midnight crosses the day.
    const late = new Date(Date.UTC(2026, 10, 18, 20, 0));
    expect(formatPattern(late, parsePattern("dd/MM HH:mm")!, { locale: "en", timeZone: "Asia/Ho_Chi_Minh" })).toBe("19/11 03:00");
  });

  it("refuses what is not a pattern", () => {
    for (const bad of ["", "  ", "dd/MM/YYYY", "yyy", "m", "'open", "'text only'", `yyyy${"-".repeat(61)}`]) {
      expect(parsePattern(bad), bad).toBeNull();
    }
    expect(parsePattern(`yyyy${"-".repeat(60)}`)).not.toBeNull();
  });
});

describe("wire values", () => {
  it("keeps dates and local date-times as written, in any time zone", () => {
    expect(formatWireDate("date", "2026-01-01", "dd/MM/yyyy")).toBe("01/01/2026");
    expect(formatWireDate("local_datetime", "2026-01-01T23:30:00", "HH:mm dd/MM/yyyy")).toBe("23:30 01/01/2026");
    expect(formatWireDate("date", "0042-03-04", "yyyy-MM-dd")).toBe("0042-03-04");
    // A zone never shows for a value that has none (the server refuses such patterns).
    expect(formatWireDate("local_datetime", "2026-01-01T23:30", "z HH:mm")).toBe(" 23:30");
  });

  it("uses the column's pattern, else the panel's, else the language's style", () => {
    expect(hasDateFormat("date")).toBe(false);
    expect(formatWireDate("date", "2026-11-18")).toBe("Nov 18, 2026");
    setDateFormats("dd/MM/yyyy", "HH:mm dd/MM/yyyy");
    expect(hasDateFormat("date")).toBe(true);
    expect(formatWireDate("date", "2026-11-18")).toBe("18/11/2026");
    expect(formatWireDate("local_datetime", "2026-11-18T09:15")).toBe("09:15 18/11/2026");
    expect(formatWireDate("date", "2026-11-18", "yyyy.MM.dd")).toBe("2026.11.18");
    // A pattern that is not one: the panel's.
    expect(formatWireDate("date", "2026-11-18", "YYYY")).toBe("18/11/2026");
    expect(formatDateTime(at)).toMatch(/^\d{2}:\d{2} \d{2}\/\d{2}\/2026$/);
  });

  it("ignores a panel pattern that is not one", () => {
    setDateFormats("nope", undefined);
    expect(hasDateFormat("date")).toBe(false);
  });
});
