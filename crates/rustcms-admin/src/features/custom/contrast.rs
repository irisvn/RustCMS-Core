//! Contrast of an application's colors (WCAG 2): a warning, never a refusal,
//! when a text color it sets (or the panel's color it lands on) falls under
//! 4.5:1. Only pairs with at least one token the application set are checked:
//! the panel's own pairs are checked by `admin-ui/e2e/layout.spec.ts`. Colors
//! the panel mixes (`color-mix`: hover and soft backgrounds, dark popovers) are
//! judged only when the application sets them.

use super::theme::{Palette, Theme};

/// Text the panel draws on a background: `(text, background)`.
const PAIRS: &[(&str, &str)] = &[
    ("foreground", "background"),
    ("foreground", "card"),
    ("foreground", "popover"),
    ("muted-foreground", "background"),
    ("muted-foreground", "card"),
    ("muted-foreground", "muted"),
    ("foreground", "selected"),
    ("primary", "card"),
    ("primary-foreground", "primary"),
    ("primary-foreground", "primary-hover"),
    ("secondary-foreground", "secondary"),
    ("destructive", "card"),
    ("destructive-foreground", "destructive"),
    ("destructive-foreground", "destructive-hover"),
    ("secondary-foreground", "secondary-hover"),
    ("success", "card"),
    ("success-foreground", "success"),
    ("warning", "card"),
    ("warning-foreground", "warning"),
    ("info", "card"),
    ("info-foreground", "info"),
    ("destructive-soft-foreground", "destructive-soft"),
    ("success-soft-foreground", "success-soft"),
    ("warning-soft-foreground", "warning-soft"),
    ("info-soft-foreground", "info-soft"),
    ("gray-soft-foreground", "gray-soft"),
];

/// WCAG AA for normal text.
const MINIMUM: f64 = 4.5;

/// The panel's light defaults (`admin-ui/src/index.css`, kept equal by a test).
pub(super) const LIGHT: &[(&str, &str)] = &[
    ("background", "#fafafa"),
    ("foreground", "#18181b"),
    ("card", "#ffffff"),
    ("popover", "var(--card)"),
    ("muted", "#f4f4f5"),
    ("muted-foreground", "#6b6b74"),
    ("subtle-foreground", "#a1a1aa"),
    ("border", "#e4e4e7"),
    ("input", "#8a8a93"),
    ("ring", "#52525b"),
    ("overlay", "#00000080"),
    ("selected", "#e4e4e7"),
    ("primary", "#18181b"),
    ("primary-foreground", "#fafafa"),
    ("secondary", "#e4e4e7"),
    ("destructive", "#b91c1c"),
    ("destructive-foreground", "#ffffff"),
    ("success", "#166534"),
    ("success-foreground", "#ffffff"),
    ("warning", "#92400e"),
    ("warning-foreground", "#ffffff"),
    ("info", "#1d4ed8"),
    ("info-foreground", "#ffffff"),
];

/// The panel's dark defaults.
pub(super) const DARK: &[(&str, &str)] = &[
    ("background", "#09090b"),
    ("foreground", "#fafafa"),
    ("card", "#111113"),
    ("muted", "#27272a"),
    ("muted-foreground", "#a1a1aa"),
    ("subtle-foreground", "#63636d"),
    ("border", "#2b2b30"),
    ("input", "#71717a"),
    ("ring", "#a1a1aa"),
    ("overlay", "#000000b3"),
    ("selected", "#34343a"),
    ("primary", "#e4e4e7"),
    ("primary-foreground", "#18181b"),
    ("secondary", "#303036"),
    ("destructive", "#f87171"),
    ("destructive-foreground", "#18181b"),
    ("success", "#4ade80"),
    ("success-foreground", "#052e16"),
    ("warning", "#fbbf24"),
    ("warning-foreground", "#18181b"),
    ("info", "#60a5fa"),
    ("info-foreground", "#172554"),
];

/// Tokens that name another in both schemes (`var(--x)`).
pub(super) const ALIASES: &[(&str, &str)] = &[
    ("secondary-foreground", "var(--foreground)"),
    ("destructive-soft-foreground", "var(--destructive)"),
    ("success-soft-foreground", "var(--success)"),
    ("warning-soft-foreground", "var(--warning)"),
    ("info-soft-foreground", "var(--info)"),
    ("gray-soft", "var(--muted)"),
    ("gray-soft-foreground", "var(--foreground)"),
];

/// A pair under [`MINIMUM`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Low {
    pub(crate) scheme: &'static str,
    pub(crate) text: &'static str,
    pub(crate) background: &'static str,
    pub(crate) ratio: f64,
}

/// The pairs of `shown` (the colors the panel shows) under 4.5:1 that involve
/// a token of `by` (who set it: the theme, or a runtime branding over it). In
/// dark mode only the dark tokens apply (the panel's dark defaults outrank the
/// light ones). Translucent colors are not judged.
pub(crate) fn low(shown: &Theme, by: (&Palette, &Palette)) -> Vec<Low> {
    let mut out = scheme("light", LIGHT, &shown.light, by.0);
    out.extend(scheme("dark", DARK, &shown.dark, by.1));
    out
}

fn scheme(name: &'static str, defaults: &[(&'static str, &'static str)], shown: &Palette, by: &Palette) -> Vec<Low> {
    let set = |palette: &Palette, token: &str| palette.colors.iter().any(|(t, _)| *t == token);
    PAIRS
        .iter()
        .filter(|(text, background)| set(by, text) || set(by, background))
        .filter_map(|&(text, background)| {
            let ratio = ratio(color(defaults, shown, text, 0)?, color(defaults, shown, background, 0)?);
            (ratio < MINIMUM).then_some(Low { scheme: name, text, background, ratio })
        })
        .collect()
}

/// `token` as shown: the application's value, else the panel's (following `var(--x)`).
fn color(defaults: &[(&str, &str)], shown: &Palette, token: &str, depth: u8) -> Option<[f64; 3]> {
    let value = shown
        .colors
        .iter()
        .find(|(t, _)| *t == token)
        .map(|(_, v)| v.as_str())
        .or_else(|| defaults.iter().chain(ALIASES).find(|(t, _)| *t == token).map(|(_, v)| *v))?;
    match value.strip_prefix("var(--").and_then(|v| v.strip_suffix(')')) {
        Some(other) if depth < 4 => color(defaults, shown, other, depth + 1),
        Some(_) => None,
        None => rgb(value),
    }
}

/// Logs each pair (`what`: "theme", "branding").
pub(crate) fn warn(what: &str, low: &[Low]) {
    for l in low {
        tracing::warn!(
            "{what}: `{}` on `{}` ({} mode) has a contrast of {:.2}:1, under the 4.5:1 text needs",
            l.text,
            l.background,
            l.scheme,
            l.ratio
        );
    }
}

/// A hex color as channels 0–1; `None` when it is translucent.
fn rgb(hex: &str) -> Option<[f64; 3]> {
    let hex = hex.strip_prefix('#')?;
    let long: String = match hex.len() {
        3 | 4 => hex.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => hex.to_owned(),
        _ => return None,
    };
    if long.len() == 8 && !long[6..].eq_ignore_ascii_case("ff") {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(long.get(i..i + 2)?, 16).ok().map(|v| f64::from(v) / 255.0);
    Some([channel(0)?, channel(2)?, channel(4)?])
}

fn luminance([r, g, b]: [f64; 3]) -> f64 {
    let linear = |v: f64| if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn ratio(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios_follow_wcag() {
        assert!((ratio(rgb("#000").unwrap(), rgb("#ffffff").unwrap()) - 21.0).abs() < 1e-9);
        assert!((ratio(rgb("#777777").unwrap(), rgb("#fff").unwrap()) - 4.48).abs() < 0.01);
        assert_eq!(rgb("#ffffff80"), None);
        assert_eq!(rgb("#fff8"), None);
        // Opaque alpha is judged.
        assert_eq!(rgb("#000000ff"), rgb("#000"));
        assert_eq!(rgb("#000f"), rgb("#000"));
    }

    fn own(theme: &Theme) -> Vec<Low> {
        low(theme, (&theme.light, &theme.dark))
    }

    #[test]
    fn only_pairs_the_application_set_are_judged() {
        assert!(own(&Theme::new()).is_empty());
        assert!(own(&Theme::new().primary("#2563eb").primary_foreground("#ffffff")).is_empty());
        // Translucent colors are not judged.
        assert!(own(&Theme::new().primary("#2563eb80")).is_empty());
        // The LMS example's theme.
        let lms = Theme::new()
            .primary("#2563eb")
            .primary_foreground("#ffffff")
            .dark(|dark| dark.primary("#60a5fa").primary_foreground("#0b1220"));
        assert!(own(&lms).is_empty());
    }

    #[test]
    fn soft_pairs_follow_aliases() {
        // A pale info background under the default text (the info color itself).
        let low = own(&Theme::new().color("info-soft", "#93c5fd"));
        assert_eq!(low.len(), 1);
        assert_eq!((low[0].text, low[0].background), ("info-soft-foreground", "info-soft"));
        // Secondary text follows foreground.
        let low = own(&Theme::new().color("secondary", "#27272a"));
        assert_eq!((low[0].text, low[0].background), ("secondary-foreground", "secondary"));
    }

    #[test]
    fn a_branding_answers_only_for_its_own_tokens() {
        let theme = Theme::new().primary("#facc15");
        let branding = Palette::default().color("muted", "#f4f4f5");
        let mut shown = theme.clone();
        shown.light = theme.light.merged(&branding);
        assert!(!own(&shown).is_empty());
        assert!(low(&shown, (&branding, &Palette::default())).is_empty());
    }

    #[test]
    fn low_pairs_are_named_with_their_scheme() {
        // Yellow buttons with the default light text on them; a pale link color.
        let theme = Theme::new().primary("#facc15").dark(|d| d.color("muted-foreground", "#3f3f46"));
        let low = own(&theme);
        let names: Vec<_> = low.iter().map(|l| (l.scheme, l.text, l.background)).collect();
        assert_eq!(
            names,
            [
                ("light", "primary", "card"),
                ("light", "primary-foreground", "primary"),
                ("dark", "muted-foreground", "background"),
                ("dark", "muted-foreground", "card"),
                ("dark", "muted-foreground", "muted"),
            ]
        );
        assert!(low.iter().all(|l| l.ratio < MINIMUM));
    }

    /// Every default pair passes, and the defaults are the stylesheet's.
    #[test]
    fn defaults_pass_and_match_the_stylesheet() {
        // Pairs with a mixed color are the E2E suite's (layout.spec.ts).
        let none = Palette::default();
        for (name, defaults) in [("light", LIGHT), ("dark", DARK)] {
            for (text, background) in PAIRS {
                let (Some(a), Some(b)) = (color(defaults, &none, text, 0), color(defaults, &none, background, 0)) else {
                    continue;
                };
                let r = ratio(a, b);
                assert!(r >= MINIMUM, "{name}: `{text}` on `{background}` is {r:.2}:1");
            }
        }
        let css = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../admin-ui/src/index.css"))
            .expect("admin-ui/src/index.css");
        let root = &css[css.find("\n:root {").expect(":root")..];
        let dark_at = root.find("@variant scheme-dark {").expect("dark block");
        let dark_end = dark_at + root[dark_at..].find("\n  }").expect("end of dark block");
        let each_at = root.find("@variant scheme-each {").expect("derived block");
        let each_end = each_at + root[each_at..].find("\n  }").expect("end of derived block");
        // Hex colors and `var(--x)`; mixed colors are not mirrored.
        let tokens = |block: &str| -> Vec<(String, String)> {
            block
                .lines()
                .filter_map(|l| l.trim().strip_prefix("--")?.strip_suffix(';')?.split_once(": "))
                .filter(|(t, v)| (v.starts_with('#') || v.starts_with("var(")) && *t != "radius")
                .map(|(t, v)| (t.to_owned(), v.to_owned()))
                .collect()
        };
        let owned = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter().map(|(t, v)| ((*t).to_owned(), (*v).to_owned())).collect()
        };
        assert_eq!(tokens(&root[..dark_at]), owned(LIGHT), "light defaults differ from index.css");
        assert_eq!(tokens(&root[dark_at..dark_end]), owned(DARK), "dark defaults differ from index.css");
        assert_eq!(tokens(&root[each_at..each_end]), owned(ALIASES), "aliases differ from index.css");
    }
}
