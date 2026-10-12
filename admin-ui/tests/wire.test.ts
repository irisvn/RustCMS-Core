import { checkText, display, fromText, localInputToInstant, toText } from "@/lib/wire";

describe("wire format", () => {
  it("keeps i64 and decimal as exact strings", () => {
    expect(fromText("i64", "9007199254740993")).toBe("9007199254740993");
    expect(fromText("decimal", "10.50")).toBe("10.50");
    expect(toText("i64", "9007199254740993")).toBe("9007199254740993");
    expect(checkText("i64", "1.5")).not.toBeNull();
    expect(checkText("decimal", "1e3")).not.toBeNull();
    expect(checkText("decimal", "-12.345")).toBeNull();
  });

  it("converts JS-safe numbers", () => {
    expect(fromText("i32", "42")).toBe(42);
    expect(fromText("f64", "0.5")).toBe(0.5);
    expect(checkText("i32", "abc")).not.toBeNull();
    expect(fromText("i32", "")).toBeNull();
  });

  it("edits instants in UTC and local date-times with seconds", () => {
    expect(localInputToInstant("2026-10-12T09:30")).toBe("2026-10-12T09:30:00Z");
    expect(localInputToInstant("2026-10-12T09:30:15")).toBe("2026-10-12T09:30:15Z");
    expect(toText("instant", "2026-10-12T09:30:00+07:00")).toBe("2026-10-12T02:30");
    expect(toText("instant", "2026-10-12T02:30:15Z")).toBe("2026-10-12T02:30:15");
    expect(toText("local_datetime", "2026-10-12T09:30:15.250")).toBe("2026-10-12T09:30:15");
    expect(fromText("local_datetime", "2026-10-12T09:30")).toBe("2026-10-12T09:30:00");
    expect(fromText("local_datetime", "2026-10-12T09:30:15")).toBe("2026-10-12T09:30:15");
    expect(checkText("date", "12/10/2026")).not.toBeNull();
  });

  it("never shifts an instant in a repeated DST hour", () => {
    // 2026-11-01 01:30 happens twice in America/New_York; editing works on UTC text.
    for (const instant of ["2026-11-01T05:30:00Z", "2026-11-01T06:30:00Z"]) {
      const text = toText("instant", instant);
      expect(fromText("instant", text)).toBe(instant);
      expect(fromText("instant", text.replace(":30", ":31"))).toBe(instant.replace(":30:", ":31:"));
    }
  });

  it("displays values", () => {
    expect(display("bool", true)).toBe("Yes");
    expect(display("string", null)).toBe("");
    expect(display("json", { a: 1 })).toBe('{"a":1}');
  });
});
