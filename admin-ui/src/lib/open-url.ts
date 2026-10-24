// URLs an action may open (`ActionOutcome.open`), checked again in the
// browser with the server's rules: `https://host…` without credentials, or an
// absolute path of this site; never `//`, whitespace, controls, backslashes or
// dot segments.

const MAX_URL = 2048;

/** `host[:port]` as the server accepts it: a DNS name or IPv4, or a bracketed IPv6; a port of 1–65535. */
function validAuthority(authority: string): boolean {
  let host = authority;
  let port: string | undefined;
  if (authority.startsWith("[")) {
    const end = authority.indexOf("]");
    if (end < 0) return false;
    const ip = authority.slice(1, end);
    const after = authority.slice(end + 1);
    if (after !== "" && !after.startsWith(":")) return false;
    port = after === "" ? undefined : after.slice(1);
    // Hex digits, colons and dots only (never `x@[::1`), and the browser's own
    // parser must read an IPv6 address (in its own spelling: `[0:0::1]` is
    // `[::1]`) with no credentials.
    if (!/^[0-9A-Fa-f:.]+$/.test(ip)) return false;
    try {
      const parsed = new URL(`https://[${ip}]/`);
      if (parsed.username !== "" || parsed.password !== "" || !/^\[[0-9a-f:.]+\]$/.test(parsed.hostname)) return false;
    } catch {
      return false;
    }
  } else {
    const colon = authority.indexOf(":");
    if (colon >= 0) {
      host = authority.slice(0, colon);
      port = authority.slice(colon + 1);
    }
    const labels = host.split(".");
    if (host === "" || host.length > 253 || !labels.every((l) => /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(l))) return false;
  }
  return port === undefined || (/^\d{1,5}$/.test(port) && Number(port) >= 1 && Number(port) <= 65535);
}

export function openableUrl(url: unknown): url is string {
  if (typeof url !== "string" || url === "" || url.length > MAX_URL) return false;
  // eslint-disable-next-line no-control-regex
  if (/[\s\u0000-\u001f\u007f-\u009f\\]/.test(url)) return false;
  if (url.slice(0, 8).toLowerCase() === "https://") {
    return validAuthority(url.slice(8).split(/[/?#]/)[0] ?? "");
  }
  const path = url.split(/[?#]/)[0] ?? "";
  const dotted = path.split("/").some((segment) => {
    const lower = segment.toLowerCase().replaceAll("%2e", ".");
    return lower === "." || lower === "..";
  });
  return url.startsWith("/") && !url.startsWith("//") && !dotted;
}

/** Opens `url` in a new tab with no access back to the panel; false when refused. */
export function openInNewTab(url: string): boolean {
  if (!openableUrl(url)) return false;
  // `noopener` makes `window.open` return null even when it opened the tab.
  window.open(url, "_blank", "noopener,noreferrer");
  return true;
}
