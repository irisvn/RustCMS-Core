// Date and time patterns (`TableColumn::date_format`, `AdminProvider::date_format`;
// the server checks them with the same grammar, engine/date_format.rs): fields
// `yyyy yy MMMM MMM MM M dd d EEEE EEE HH H hh h mm ss a z`, text between single
// quotes (`''` a quote), other characters as they are. Names (months, weekdays,
// AM/PM, the zone) are in the panel's language; digits are Latin.

/** A field of a pattern: its letter, repeated `len` times. */
export interface DateField {
  letter: string;
  len: number;
}

/** Text kept as it is, or a field. */
export type DateToken = string | DateField;

const FIELDS: Record<string, readonly number[]> = {
  y: [2, 4],
  M: [1, 2, 3, 4],
  d: [1, 2],
  E: [3, 4],
  H: [1, 2],
  h: [1, 2],
  m: [2],
  s: [2],
  a: [1],
  z: [1],
};

/** Longest pattern, in characters (as the server). */
export const MAX_PATTERN = 64;

/** The tokens of `pattern`, or `null` when it is not one (the default is used then). */
export function parsePattern(pattern: string): DateToken[] | null {
  const chars = Array.from(pattern);
  if (!pattern.trim() || chars.length > MAX_PATTERN) return null;
  const tokens: DateToken[] = [];
  const text = (s: string) => {
    const last = tokens[tokens.length - 1];
    if (typeof last === "string") tokens[tokens.length - 1] = last + s;
    else tokens.push(s);
  };
  let fields = 0;
  for (let i = 0; i < chars.length; ) {
    const c = chars[i] ?? "";
    if (c === "'") {
      if (chars[i + 1] === "'") {
        text("'");
        i += 2;
        continue;
      }
      let j = i + 1;
      let quoted = "";
      for (;;) {
        if (j >= chars.length) return null;
        if (chars[j] === "'" && chars[j + 1] === "'") {
          quoted += "'";
          j += 2;
        } else if (chars[j] === "'") {
          break;
        } else {
          quoted += chars[j];
          j += 1;
        }
      }
      text(quoted);
      i = j + 1;
      continue;
    }
    if (!/[A-Za-z]/.test(c)) {
      text(c);
      i += 1;
      continue;
    }
    let len = 1;
    while (chars[i + len] === c) len += 1;
    if (!FIELDS[c]?.includes(len)) return null;
    tokens.push({ letter: c, len });
    fields += 1;
    i += len;
  }
  return fields > 0 ? tokens : null;
}

const formatters = new Map<string, Intl.DateTimeFormat>();

/** A cached formatter (they are costly to build). */
function formatter(locale: string, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let found = formatters.get(key);
  if (!found) {
    found = new Intl.DateTimeFormat(locale, options);
    formatters.set(key, found);
  }
  return found;
}

/** The part of `type` of `date`'s formatting with `options`. */
function part(date: Date, locale: string, options: Intl.DateTimeFormatOptions, type: Intl.DateTimeFormatPartTypes): string {
  return formatter(locale, options).formatToParts(date).find((p) => p.type === type)?.value ?? "";
}

const pad = (n: number, width: number) => String(n).padStart(width, "0");

/**
 * `date` written with `tokens` in `locale`, in `timeZone` (the browser's when
 * absent). `zone`: whether a `z` field shows the time zone (instants only).
 */
export function formatPattern(
  date: Date,
  tokens: readonly DateToken[],
  { locale, timeZone, zone = true }: { locale: string; timeZone?: string; zone?: boolean },
): string {
  const tz = timeZone ? { timeZone } : {};
  // Numbers in Latin digits from one formatter; names in the user's language.
  const numbers = formatter("en-US", {
    ...tz, year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", second: "numeric", hourCycle: "h23",
  }).formatToParts(date);
  const n = (type: Intl.DateTimeFormatPartTypes) => Number(numbers.find((p) => p.type === type)?.value ?? 0);
  // `formatToParts` writes years as positive numbers (no era): the calendar year.
  const year = timeZone === "UTC" ? date.getUTCFullYear() : n("year");
  const hour = n("hour") % 24;
  let out = "";
  for (const token of tokens) {
    if (typeof token === "string") {
      out += token;
      continue;
    }
    const { letter, len } = token;
    switch (letter) {
      case "y":
        out += len === 2 ? pad(Math.abs(year) % 100, 2) : pad(year, 4);
        break;
      case "M":
        out += len >= 3 ? part(date, locale, { ...tz, month: len === 4 ? "long" : "short" }, "month") : pad(n("month"), len);
        break;
      case "d":
        out += pad(n("day"), len);
        break;
      case "E":
        out += part(date, locale, { ...tz, weekday: len === 4 ? "long" : "short" }, "weekday");
        break;
      case "H":
        out += pad(hour, len);
        break;
      case "h":
        out += pad(hour % 12 || 12, len);
        break;
      case "m":
        out += pad(n("minute"), 2);
        break;
      case "s":
        out += pad(n("second"), 2);
        break;
      case "a":
        out += part(date, locale, { ...tz, hour: "numeric", hour12: true }, "dayPeriod");
        break;
      case "z":
        if (zone) out += part(date, locale, { ...tz, timeZoneName: "short" }, "timeZoneName");
        break;
    }
  }
  return out;
}
