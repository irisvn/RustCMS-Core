//! The sub-path of a custom page (`/p/{slug}/{*path}`, ADR 0032): read from
//! the raw request path, split before decoding, each segment decoded once.

/// Longest raw sub-path.
const MAX_BYTES: usize = 512;
/// Most segments.
const MAX_SEGMENTS: usize = 16;

/// The canonical sub-path (`containers/web/logs`) of `raw` (still encoded,
/// without the leading slash), or `None` when it is not one: malformed or
/// double escapes, invalid UTF-8, an encoded `/` or `\`, `.` or `..`, an
/// empty segment, too long, or a character outside `[A-Za-z0-9._~-]`.
pub(crate) fn canonical(raw: &str) -> Option<String> {
    let raw = raw.strip_suffix('/').unwrap_or(raw);
    if raw.is_empty() {
        return Some(String::new());
    }
    if raw.len() > MAX_BYTES {
        return None;
    }
    let segments: Vec<&str> = raw.split('/').collect();
    if segments.len() > MAX_SEGMENTS {
        return None;
    }
    let mut out = Vec::with_capacity(segments.len());
    for segment in segments {
        let decoded = decode_once(segment)?;
        let ok = !decoded.is_empty()
            && decoded != "."
            && decoded != ".."
            && decoded.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'));
        if !ok {
            return None;
        }
        out.push(decoded);
    }
    Some(out.join("/"))
}

/// One pass of percent-decoding; `None` on a malformed escape or invalid
/// UTF-8. A remaining `%` fails the character check above (double encoding).
fn decode_once(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let value = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::canonical;

    #[test]
    fn accepts_plain_and_once_encoded_segments() {
        assert_eq!(canonical("").as_deref(), Some(""));
        assert_eq!(canonical("/").as_deref(), Some(""));
        assert_eq!(canonical("containers/web/logs").as_deref(), Some("containers/web/logs"));
        assert_eq!(canonical("containers/web/").as_deref(), Some("containers/web"));
        assert_eq!(canonical("a%2Db").as_deref(), Some("a-b"));
        assert_eq!(canonical("v1.2~x_y").as_deref(), Some("v1.2~x_y"));
    }

    #[test]
    fn refuses_traps() {
        for bad in [
            "a%2Fb",
            "a%5Cb",
            "a\\b",
            "..",
            ".",
            "a/../b",
            "%2e%2e",
            "%2E.",
            "%252e",
            "a%2",
            "a%zz",
            "a//b",
            "%C3%28",
            "caf%C3%A9",
            "a b",
            "a?b",
            "a#b",
        ] {
            assert_eq!(canonical(bad), None, "{bad}");
        }
        assert_eq!(canonical(&"a/".repeat(17)), None, "too many segments");
        assert_eq!(canonical(&"a".repeat(513)), None, "too long");
    }
}
