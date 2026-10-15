//! The panel's look (`AdminProvider::theme`): color tokens, corner radius,
//! system font stack, logo and favicon, light/dark. Every value that reaches
//! the generated stylesheet is checked at boot: no text of the application is
//! ever copied into CSS as written.

use std::fmt::Write;

/// Color tokens an application may set (the panel's stable theme API, version 1).
pub const TOKENS: &[&str] = &[
    "background",
    "foreground",
    "card",
    "muted",
    "muted-foreground",
    "border",
    "ring",
    "primary",
    "primary-foreground",
    "secondary",
    "destructive",
    "destructive-foreground",
    "success",
    "warning",
];

/// Largest logo or favicon.
const MAX_IMAGE_BYTES: usize = 512 * 1024;

/// Light, dark, or as the operating system prefers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorScheme {
    /// Follow the operating system (default).
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

impl ColorScheme {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// Token values of one color scheme.
#[derive(Debug, Clone, Default)]
pub struct Palette {
    colors: Vec<(&'static str, String)>,
}

impl Palette {
    /// Sets the color token `token` (one of [`TOKENS`]) to a hex color.
    ///
    /// # Panics
    /// On an unknown token or a value other than `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
    pub fn color(mut self, token: &str, value: &str) -> Self {
        let token =
            TOKENS.iter().copied().find(|t| *t == token).unwrap_or_else(|| panic!("theme: unknown token `{token}`"));
        assert!(hex_color(value), "theme: `{token}` must be a hex color (#rgb, #rrggbb, …), not `{value}`");
        self.colors.retain(|(t, _)| *t != token);
        self.colors.push((token, value.to_ascii_lowercase()));
        self
    }

    /// `--primary`.
    pub fn primary(self, value: &str) -> Self {
        self.color("primary", value)
    }

    /// `--primary-foreground` (text on primary).
    pub fn primary_foreground(self, value: &str) -> Self {
        self.color("primary-foreground", value)
    }
}

/// An image the panel serves (logo, favicon).
#[derive(Debug, Clone)]
pub(crate) struct Image {
    pub(crate) bytes: &'static [u8],
    pub(crate) mime: &'static str,
}

/// The panel's look.
#[derive(Debug, Clone, Default)]
pub struct Theme {
    light: Palette,
    dark: Palette,
    radius: Option<String>,
    font: Option<String>,
    pub(crate) logo: Option<Image>,
    pub(crate) favicon: Option<Image>,
    pub(crate) scheme: ColorScheme,
}

impl Theme {
    /// The default look.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a color token for both schemes (override dark ones with [`Theme::dark`]).
    pub fn color(mut self, token: &str, value: &str) -> Self {
        self.light = self.light.color(token, value);
        self
    }

    /// `--primary`.
    pub fn primary(self, value: &str) -> Self {
        self.color("primary", value)
    }

    /// `--primary-foreground` (text on primary).
    pub fn primary_foreground(self, value: &str) -> Self {
        self.color("primary-foreground", value)
    }

    /// Tokens of the dark scheme.
    pub fn dark(mut self, palette: impl FnOnce(Palette) -> Palette) -> Self {
        self.dark = palette(std::mem::take(&mut self.dark));
        self
    }

    /// Corner radius: `0.75rem`, `8px` (at most 4rem / 64px).
    ///
    /// # Panics
    /// On any other value.
    pub fn radius(mut self, value: &str) -> Self {
        assert!(length(value), "theme: radius must be a number of rem (≤ 4) or px (≤ 64), not `{value}`");
        self.radius = Some(value.to_owned());
        self
    }

    /// Font families, best first (installed fonts: `Inter, system-ui, sans-serif`).
    /// Each name is quoted in the stylesheet; generic families stay bare.
    ///
    /// # Panics
    /// On names other than letters, digits, spaces and dashes, or more than 200 characters.
    pub fn font_family(mut self, families: &str) -> Self {
        assert!(families.len() <= 200, "theme: font_family is at most 200 characters");
        let mut out = Vec::new();
        for family in families.split(',').map(str::trim) {
            assert!(
                !family.is_empty() && family.bytes().all(|b| b.is_ascii_alphanumeric() || b == b' ' || b == b'-'),
                "theme: font family `{family}`: letters, digits, spaces and dashes only"
            );
            const GENERIC: &[&str] =
                &["serif", "sans-serif", "monospace", "system-ui", "ui-sans-serif", "ui-serif", "ui-monospace"];
            out.push(if GENERIC.contains(&family) { family.to_owned() } else { format!("\"{family}\"") });
        }
        self.font = Some(out.join(", "));
        self
    }

    /// The logo shown above the navigation: PNG, WebP or ICO (≤ 512 KiB).
    ///
    /// # Panics
    /// On another type, a type its bytes do not match, or a larger file.
    pub fn logo(mut self, bytes: &'static [u8], mime: &'static str) -> Self {
        self.logo = Some(image("logo", bytes, mime));
        self
    }

    /// The browser tab icon: PNG, WebP or ICO (≤ 512 KiB).
    ///
    /// # Panics
    /// As [`Theme::logo`].
    pub fn favicon(mut self, bytes: &'static [u8], mime: &'static str) -> Self {
        self.favicon = Some(image("favicon", bytes, mime));
        self
    }

    /// Light, dark, or as the operating system prefers (default).
    pub fn color_scheme(mut self, scheme: ColorScheme) -> Self {
        self.scheme = scheme;
        self
    }

    /// The stylesheet loaded after the panel's own, or `None` for the default look.
    pub(crate) fn stylesheet(&self) -> Option<String> {
        let mut root = String::new();
        for (token, value) in &self.light.colors {
            let _ = write!(root, "--{token}:{value};");
        }
        if let Some(radius) = &self.radius {
            let _ = write!(root, "--radius:{radius};");
        }
        if let Some(font) = &self.font {
            let _ = write!(root, "--font-sans:{font};");
        }
        let mut dark = String::new();
        for (token, value) in &self.dark.colors {
            let _ = write!(dark, "--{token}:{value};");
        }
        if root.is_empty() && dark.is_empty() {
            return None;
        }
        let mut css = String::new();
        if !root.is_empty() {
            let _ = write!(css, ":root{{{root}}}");
        }
        if !dark.is_empty() {
            let _ = write!(css, ":root[data-scheme=dark]{{{dark}}}");
        }
        Some(css)
    }
}

fn hex_color(value: &str) -> bool {
    value
        .strip_prefix('#')
        .is_some_and(|hex| matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn length(value: &str) -> bool {
    let (number, max) = if let Some(n) = value.strip_suffix("rem") {
        (n, 4.0)
    } else if let Some(n) = value.strip_suffix("px") {
        (n, 64.0)
    } else {
        return false;
    };
    let shaped = !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && number.matches('.').count() <= 1
        && !number.starts_with('.')
        && !number.ends_with('.');
    shaped && number.parse::<f64>().is_ok_and(|n| (0.0..=max).contains(&n))
}

fn image(what: &str, bytes: &'static [u8], mime: &'static str) -> Image {
    assert!(bytes.len() <= MAX_IMAGE_BYTES, "theme: the {what} is larger than 512 KiB");
    let detected = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/webp" => bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        "image/x-icon" | "image/vnd.microsoft.icon" => bytes.starts_with(&[0, 0, 1, 0]),
        _ => panic!("theme: the {what} must be PNG, WebP or ICO, not `{mime}`"),
    };
    assert!(detected, "theme: the {what}'s bytes are not {mime}");
    Image { bytes, mime }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn values_reach_css_only_when_checked() {
        let theme = Theme::new()
            .primary("#2563EB")
            .radius("0.75rem")
            .font_family("Inter, system-ui, sans-serif")
            .dark(|d| d.primary("#60a5fa"));
        assert_eq!(
            theme.stylesheet().unwrap(),
            ":root{--primary:#2563eb;--radius:0.75rem;--font-sans:\"Inter\", system-ui, sans-serif;}\
             :root[data-scheme=dark]{--primary:#60a5fa;}"
        );
        assert!(Theme::new().stylesheet().is_none());
    }

    #[test]
    fn hostile_values_stop_the_boot() {
        let fails = |f: fn() -> Theme| std::panic::catch_unwind(f).is_err();
        assert!(fails(|| Theme::new().primary("red;}body{display:none")));
        assert!(fails(|| Theme::new().primary("#12345")));
        assert!(fails(|| Theme::new().color("--x", "#fff")));
        assert!(fails(|| Theme::new().radius("1rem;}")));
        assert!(fails(|| Theme::new().radius("100px")));
        assert!(fails(|| Theme::new().radius(".5rem")));
        assert!(fails(|| Theme::new().font_family("Inter\"; } body { x: \"")));
        assert!(fails(|| Theme::new().font_family("url(x)")));
        assert!(fails(|| Theme::new().logo(b"<svg onload=alert(1)>", "image/svg+xml")));
        assert!(fails(|| Theme::new().logo(b"GIF89a", "image/png")));
        assert!(!fails(|| Theme::new().logo(PNG, "image/png").radius("8px").radius("0rem")));
    }
}
