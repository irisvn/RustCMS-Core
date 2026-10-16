//! `[auth]` configuration section (`config/auth.toml`, `RUSTCMS_AUTH__*`).

use serde::Deserialize;

/// Authentication settings. Every field has a default.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// URL prefix of the admin panel; the session cookie is scoped to it.
    pub admin_path: String,
    /// Origins allowed to send state-changing admin requests. Empty: the origin of `app.url`.
    pub admin_origins: Vec<String>,
    /// Session cookie name.
    pub cookie_name: String,
    /// `Secure` cookie flag. Default: when `app.url` is https.
    pub secure_cookies: Option<bool>,
    /// Idle session lifetime in minutes (sliding).
    pub session_lifetime_minutes: u64,
    /// Lifetime of anonymous sessions (only a CSRF token, before sign-in).
    pub anonymous_session_lifetime_minutes: u64,
    /// New anonymous sessions allowed per IP per minute.
    pub anonymous_sessions_per_ip: u32,
    /// New anonymous sessions allowed per minute in total (bounds storage:
    /// at most this × `anonymous_session_lifetime_minutes` live anonymous rows).
    pub anonymous_sessions_per_minute: u32,
    /// Minimum password length.
    pub password_min_length: usize,
    /// Revoke API tokens when the password changes.
    pub revoke_tokens_on_password_change: bool,
    /// Failed sign-ins allowed per account and IP per minute.
    pub attempts_per_identifier: u32,
    /// Failed sign-ins allowed per IP per minute (any account).
    pub attempts_per_ip: u32,
    /// Seconds a user's resolved permissions are cached (changes also evict immediately).
    pub permission_cache_ttl_secs: u64,
    /// JWT access and refresh tokens of the API guard (`[auth.jwt]`).
    pub jwt: JwtConfig,
    /// "Forgot password?": reset links by mail (needs a mail driver).
    pub password_reset: bool,
    /// How long a reset link works, in minutes.
    pub password_reset_minutes: u64,
    /// How long an invitation link works, in days.
    pub invitation_days: u64,
    /// Reset requests per IP per minute.
    pub reset_requests_per_ip: u32,
    /// Reset requests per email per minute.
    pub reset_requests_per_email: u32,
    /// A new reset mail for the same user is sent at most this often (seconds).
    pub reset_cooldown_secs: u64,
    /// "Remember me" keeps a session this many days, across browser restarts
    /// (0: no such option; sessions end when the browser closes).
    pub remember_days: u64,
}

/// `[auth.jwt]`: JWT access tokens with rotating refresh tokens.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct JwtConfig {
    /// Whether the JWT endpoints and bearer JWTs are accepted.
    pub enabled: bool,
    /// Access token lifetime in seconds (60 to 3,600).
    pub access_ttl_secs: u64,
    /// A refresh token not used for this many days stops working (1 to 90).
    pub refresh_ttl_days: u64,
    /// A sign-in (refresh family) ends after this many days, however often it is
    /// refreshed (at least `refresh_ttl_days`, at most 365).
    pub refresh_family_days: u64,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self { enabled: true, access_ttl_secs: 900, refresh_ttl_days: 14, refresh_family_days: 30 }
    }
}

impl JwtConfig {
    /// The bounds above, or what is wrong.
    pub fn validate(&self) -> Result<(), String> {
        if !(60..=3_600).contains(&self.access_ttl_secs) {
            return Err("auth.jwt.access_ttl_secs must be between 60 and 3600".into());
        }
        if !(1..=90).contains(&self.refresh_ttl_days) {
            return Err("auth.jwt.refresh_ttl_days must be between 1 and 90".into());
        }
        if !(self.refresh_ttl_days..=365).contains(&self.refresh_family_days) {
            return Err("auth.jwt.refresh_family_days must be between refresh_ttl_days and 365".into());
        }
        Ok(())
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            admin_path: "/admin".into(),
            admin_origins: Vec::new(),
            cookie_name: "rustcms_session".into(),
            secure_cookies: None,
            session_lifetime_minutes: 120,
            anonymous_session_lifetime_minutes: 15,
            anonymous_sessions_per_ip: 30,
            anonymous_sessions_per_minute: 1_000,
            password_min_length: 8,
            revoke_tokens_on_password_change: true,
            attempts_per_identifier: 5,
            attempts_per_ip: 30,
            permission_cache_ttl_secs: 30,
            jwt: JwtConfig::default(),
            password_reset: false,
            password_reset_minutes: 60,
            invitation_days: 7,
            reset_requests_per_ip: 5,
            reset_requests_per_email: 3,
            reset_cooldown_secs: 60,
            remember_days: 30,
        }
    }
}

/// Validates and normalizes an admin path: `/segment(/segment)*` with segments of
/// `[A-Za-z0-9_-]`, no trailing slash. The site root and `/api…` are refused (the
/// whole site would use the admin session guard, or collide with the API).
pub fn normalize_admin_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim().trim_end_matches('/');
    let segments: Vec<&str> = trimmed.strip_prefix('/').map(|p| p.split('/').collect()).unwrap_or_default();
    let valid = !segments.is_empty()
        && segments
            .iter()
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));
    if !valid {
        return Err(format!("auth.admin_path `{path}` must look like `/admin` (letters, digits, `_`, `-`)"));
    }
    if segments[0] == "api" {
        return Err(format!("auth.admin_path `{path}` may not be under `/api`"));
    }
    Ok(format!("/{}", segments.join("/")))
}

/// `scheme://host[:port]` of a URL, lowercased; `None` if it has no scheme.
pub(crate) fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (!authority.is_empty()).then(|| format!("{}://{}", scheme.to_ascii_lowercase(), authority.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_paths() {
        assert_eq!(normalize_admin_path("/admin").unwrap(), "/admin");
        assert_eq!(normalize_admin_path("/admin/").unwrap(), "/admin");
        assert_eq!(normalize_admin_path("/back-office/panel_1").unwrap(), "/back-office/panel_1");
        for bad in ["", "/", "admin", "//admin", "/admin//x", "/ad min", "/admin\"x", "/api", "/api/admin", "/a<b>"] {
            assert!(normalize_admin_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn origins() {
        assert_eq!(origin_of("https://Example.com/admin?x=1").as_deref(), Some("https://example.com"));
        assert_eq!(origin_of("http://localhost:8000").as_deref(), Some("http://localhost:8000"));
        assert_eq!(origin_of("example.com"), None);
        assert_eq!(origin_of("https://"), None);
    }
}
