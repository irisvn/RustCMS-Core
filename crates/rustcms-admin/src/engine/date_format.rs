//! Date and time patterns (`TableColumn::date_format`, `Entry::date_format`,
//! `AdminProvider::date_format`): checked here at boot, applied by the panel in
//! the viewer's language and time zone (`admin-ui/src/i18n/dates.ts`, the same
//! grammar).
//!
//! Letters are fields: `yyyy` `yy` (year), `MMMM` `MMM` `MM` `M` (month: long
//! and short names, two digits, digits), `dd` `d` (day), `EEEE` `EEE`
//! (weekday), `HH` `H` (hour 0–23), `hh` `h` (hour 1–12), `mm` (minute), `ss`
//! (second), `a` (AM/PM), `z` (time zone). Text between single quotes is kept
//! (`''` is a quote); other characters are kept as they are. Any other letter
//! outside quotes is refused.

use crate::wire::FieldKind;

/// Longest pattern, in characters.
pub(crate) const MAX_LEN: usize = 64;

/// Fields a pattern may use, by letter: the run lengths allowed.
const FIELDS: [(char, &[usize]); 10] = [
    ('y', &[2, 4]),
    ('M', &[1, 2, 3, 4]),
    ('d', &[1, 2]),
    ('E', &[3, 4]),
    ('H', &[1, 2]),
    ('h', &[1, 2]),
    ('m', &[2]),
    ('s', &[2]),
    ('a', &[1]),
    ('z', &[1]),
];

/// A field of a pattern: its letter and how many times it is repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Field {
    pub(crate) letter: char,
    pub(crate) len: usize,
}

/// The fields of `pattern`, or why it is not a pattern.
pub(crate) fn parse(pattern: &str) -> Result<Vec<Field>, String> {
    if pattern.trim().is_empty() {
        return Err("the pattern is empty".to_owned());
    }
    if pattern.chars().count() > MAX_LEN {
        return Err(format!("the pattern is longer than {MAX_LEN} characters"));
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut fields = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            // Quoted text, `''` a quote; an unclosed quote is refused.
            if chars.get(i + 1) == Some(&'\'') {
                i += 2;
                continue;
            }
            let mut j = i + 1;
            loop {
                match chars.get(j) {
                    None => return Err("a quote is not closed".to_owned()),
                    Some('\'') if chars.get(j + 1) == Some(&'\'') => j += 2,
                    Some('\'') => break,
                    Some(_) => j += 1,
                }
            }
            i = j + 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let len = chars[i..].iter().take_while(|&&n| n == c).count();
        let Some((_, lengths)) = FIELDS.iter().find(|(letter, _)| *letter == c) else {
            return Err(format!("`{c}` is not a field (text goes between quotes: 'at')"));
        };
        if !lengths.contains(&len) {
            return Err(format!("`{}` is not a field", c.to_string().repeat(len)));
        }
        fields.push(Field { letter: c, len });
        i += len;
    }
    if fields.is_empty() {
        return Err("the pattern shows no field".to_owned());
    }
    Ok(fields)
}

/// Whether a pattern of `fields` fits values of `kind`: dates have no time,
/// local date-times no time zone; other kinds take no pattern.
pub(crate) fn fits(fields: &[Field], kind: FieldKind) -> Result<(), String> {
    let time = |f: &&Field| matches!(f.letter, 'H' | 'h' | 'm' | 's' | 'a' | 'z');
    match kind {
        FieldKind::Date => match fields.iter().find(time) {
            Some(f) => Err(format!("a date has no time (`{}`)", f.letter.to_string().repeat(f.len))),
            None => Ok(()),
        },
        FieldKind::LocalDateTime if fields.iter().any(|f| f.letter == 'z') => {
            Err("a local date-time has no time zone (`z`)".to_owned())
        }
        FieldKind::LocalDateTime | FieldKind::Instant => Ok(()),
        other => Err(format!("a {} field is not a date", other.name())),
    }
}

/// Panics with `owner`'s name unless `pattern` is a pattern fitting `kind`
/// (any date kind when `None`, as a panel default).
pub(crate) fn check(owner: &str, pattern: &str, kind: Option<FieldKind>) {
    let fields = parse(pattern).unwrap_or_else(|e| panic!("{owner}: date format {pattern:?}: {e}"));
    if let Some(kind) = kind {
        fits(&fields, kind).unwrap_or_else(|e| panic!("{owner}: date format {pattern:?}: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letters(pattern: &str) -> Vec<String> {
        parse(pattern).unwrap().iter().map(|f| f.letter.to_string().repeat(f.len)).collect()
    }

    #[test]
    fn every_field_is_read() {
        assert_eq!(letters("EEEE, d MMMM yyyy 'at' HH:mm:ss"), ["EEEE", "d", "MMMM", "yyyy", "HH", "mm", "ss"]);
        assert_eq!(letters("EEE dd/MM/yy h:mm a z"), ["EEE", "dd", "MM", "yy", "h", "mm", "a", "z"]);
        assert_eq!(letters("M/d MMM H"), ["M", "d", "MMM", "H"]);
    }

    #[test]
    fn quoted_text_is_kept() {
        assert_eq!(letters("'lúc' HH:mm dd/MM/yyyy"), ["HH", "mm", "dd", "MM", "yyyy"]);
        assert_eq!(letters("dd''MM"), ["dd", "MM"]);
        assert_eq!(letters("'it''s' yyyy"), ["yyyy"]);
    }

    #[test]
    fn others_are_refused() {
        for (pattern, error) in [
            ("dd/MM/YYYY", "`Y` is not a field"),
            ("at HH:mm", "`t` is not a field"),
            ("yyy", "`yyy` is not a field"),
            ("ddd", "`ddd` is not a field"),
            ("m", "`m` is not a field"),
            ("MMMMM", "`MMMMM` is not a field"),
            ("'open", "a quote is not closed"),
            ("", "the pattern is empty"),
            ("  ", "the pattern is empty"),
            ("'text only'", "the pattern shows no field"),
        ] {
            let found = parse(pattern).unwrap_err();
            assert!(found.starts_with(error), "{pattern}: {found}");
        }
        let long = format!("yyyy{}", "-".repeat(MAX_LEN - 3));
        assert!(parse(&long).unwrap_err().contains("longer than 64"));
        assert!(parse(&long[..MAX_LEN]).is_ok());
    }

    #[test]
    fn patterns_fit_their_kind() {
        let p = |s| parse(s).unwrap();
        assert!(fits(&p("dd/MM/yyyy"), FieldKind::Date).is_ok());
        assert_eq!(fits(&p("dd/MM/yyyy HH:mm"), FieldKind::Date).unwrap_err(), "a date has no time (`HH`)");
        assert_eq!(fits(&p("d a"), FieldKind::Date).unwrap_err(), "a date has no time (`a`)");
        assert!(fits(&p("HH:mm dd/MM/yyyy"), FieldKind::LocalDateTime).is_ok());
        assert_eq!(
            fits(&p("HH:mm z"), FieldKind::LocalDateTime).unwrap_err(),
            "a local date-time has no time zone (`z`)"
        );
        assert!(fits(&p("HH:mm z"), FieldKind::Instant).is_ok());
        assert_eq!(fits(&p("yyyy"), FieldKind::String).unwrap_err(), "a string field is not a date");
    }
}
