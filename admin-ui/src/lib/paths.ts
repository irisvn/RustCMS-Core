// Paths extensions give to the panel (ADR 0032): `api` and `realtime` paths
// under `{admin}/api/x`, and the sub-path of a custom page. One validator, so
// an extension bug fails closed (extension code is trusted; this is not a
// sandbox).

const SEGMENT = /^[A-Za-z0-9._~-]+$/;

/** One pass of percent-decoding a segment, or `null` (malformed, invalid UTF-8). */
function decodeOnce(segment: string): string | null {
  if (/%(?![0-9A-Fa-f]{2})/.test(segment)) return null;
  try {
    return decodeURIComponent(segment);
  } catch {
    return null;
  }
}

function checkSegment(segment: string): string | null {
  const decoded = decodeOnce(segment);
  if (decoded === null || decoded === "" || decoded === "." || decoded === "..") return null;
  // A decoded `/`, `\` or a remaining `%` (double encoding) fails here.
  return SEGMENT.test(decoded) ? decoded : null;
}

/**
 * An extension path (`/docker/containers/web`): exactly one leading slash,
 * no scheme, `//`, `\`, controls, `?`, `#` or empty segments; each segment
 * decoded once to `[A-Za-z0-9._~-]+`. Returns it re-encoded, segment by
 * segment, or throws.
 */
export function extensionPath(path: string): string {
  const bad = () => new Error(`invalid extension path ${JSON.stringify(path)}`);
  // eslint-disable-next-line no-control-regex
  if (typeof path !== "string" || !path.startsWith("/") || path.startsWith("//") || /[\\?#\u0000-\u001f\u007f]/.test(path)) {
    throw bad();
  }
  const segments = path.slice(1).split("/");
  const out: string[] = [];
  for (const segment of segments) {
    const decoded = checkSegment(segment);
    if (decoded === null) throw bad();
    out.push(encodeURIComponent(decoded));
  }
  return `/${out.join("/")}`;
}

/**
 * The URL of extension path `path` under `apiPath` (`/admin/api`, the
 * panel's own, from the server): it keeps `apiPath`'s origin and stays under
 * `{apiPath}/x/` at a segment boundary, or this throws.
 */
export function extensionUrl(apiPath: string, path: string, origin = globalThis.location?.origin ?? "http://localhost"): string {
  const canonical = extensionPath(path);
  const prefix = `${apiPath.replace(/\/$/, "")}/x`;
  const base = new URL(prefix, origin);
  const url = new URL(`${prefix}${canonical}`, origin);
  if (url.origin !== base.origin || !url.pathname.startsWith(`${base.pathname}/`)) {
    throw new Error(`invalid extension path ${JSON.stringify(path)}`);
  }
  return `${prefix}${canonical}`;
}

/** Longest raw sub-path and most segments (as the server). */
const SUB_MAX_BYTES = 512;
const SUB_MAX_SEGMENTS = 16;

/**
 * The canonical sub-path of a custom page from its raw (still encoded)
 * form, `""` for none, or `null` when it is not one (the server's rules).
 */
export function canonicalSubPath(raw: string): string | null {
  const trimmed = raw.endsWith("/") ? raw.slice(0, -1) : raw;
  if (trimmed === "") return "";
  if (new TextEncoder().encode(trimmed).length > SUB_MAX_BYTES) return null;
  const segments = trimmed.split("/");
  if (segments.length > SUB_MAX_SEGMENTS) return null;
  const out: string[] = [];
  for (const segment of segments) {
    const decoded = checkSegment(segment);
    if (decoded === null) return null;
    out.push(decoded);
  }
  return out.join("/");
}

/** A canonical sub-path, encoded segment by segment for a request. */
export function encodeSubPath(path: string): string {
  return path === "" ? "" : path.split("/").map(encodeURIComponent).join("/");
}
