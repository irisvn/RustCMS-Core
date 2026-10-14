//! What an upload is: its type from its content (fixed allowlist), whether the
//! file is complete, its name and how it is served.

use super::*;

/// MIME types the panel may show inline, with their file extensions. Anything
/// else is served as an attachment; nothing outside this map is accepted.
pub(super) const TYPES: [(&str, &str, bool); 5] = [
    ("image/png", "png", true),
    ("image/jpeg", "jpg", true),
    ("image/gif", "gif", true),
    ("image/webp", "webp", true),
    ("application/pdf", "pdf", false),
];

/// Longest kept original file name, in characters.
pub(super) const MAX_NAME: usize = 255;

pub(super) fn inline(mime: &str) -> bool {
    TYPES.iter().any(|(t, _, inline)| *t == mime && *inline)
}

/// A file name safe to keep and to send back: no path, controls, bidi marks or
/// quotes; at most 255 characters; never empty.
pub fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '"' | '\\' | ';' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(MAX_NAME)
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() { "file".to_owned() } else { trimmed.to_owned() }
}

/// `Content-Disposition` for a stored file: inline for images, attachment otherwise,
/// with an ASCII fallback and the UTF-8 name (RFC 6266 / 5987).
pub(super) fn disposition(name: &str, inline: bool) -> HeaderValue {
    let ascii: String = name
        .chars()
        .map(|c| if (c.is_ascii_graphic() || c == ' ') && c != '"' && c != '\\' { c } else { '_' })
        .collect();
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let kind = if inline { "inline" } else { "attachment" };
    HeaderValue::from_str(&format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// The detected type of `head` (magic bytes), if accepted.
pub(super) fn detect(head: &[u8], allowed: &[String]) -> Option<(&'static str, &'static str)> {
    let kind = infer::get(head)?;
    TYPES
        .iter()
        .find(|(mime, _, _)| *mime == kind.mime_type() && allowed.iter().any(|a| a == mime))
        .map(|(mime, ext, _)| (*mime, *ext))
}

/// Whether the file ends as its type requires (a header alone is not a file):
/// PNG with `IEND`, JPEG with `FFD9`, GIF with `;`, WebP with a RIFF size
/// matching the file, PDF with `%%EOF` near the end.
pub(super) fn complete(mime: &str, path: &Path, size: u64) -> AppResult<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(AppError::internal)?;
    let mut head = [0u8; 12];
    let n = file.read(&mut head).map_err(AppError::internal)?;
    let tail_len = size.min(1024);
    file.seek(SeekFrom::End(-(tail_len as i64))).map_err(AppError::internal)?;
    let mut tail = vec![0u8; tail_len as usize];
    file.read_exact(&mut tail).map_err(AppError::internal)?;
    let ends = |suffix: &[u8]| tail.ends_with(suffix);
    Ok(match mime {
        "image/png" => ends(b"IEND\xaeB`\x82"),
        "image/jpeg" => ends(b"\xff\xd9"),
        "image/gif" => ends(b";"),
        "image/webp" => n >= 8 && u64::from(u32::from_le_bytes([head[4], head[5], head[6], head[7]])) + 8 == size,
        "application/pdf" => tail.windows(5).any(|w| w == b"%%EOF"),
        _ => false,
    })
}
