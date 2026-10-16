//! The Mail settings page (`AdminProvider::mail_settings`): the SMTP server
//! of [`MailSettings`], checked against the operator's policy (`[mail]`), the
//! password kept only while host, port and user name stay the same, and
//! `POST settings/mail/test` sending a test mail with the saved settings.

use async_trait::async_trait;
use axum::extract::{Request, State};
use rustcms_core::mail::{self, Encryption, MailSettings, Message};
use rustcms_core::{AppError, AppResult, AppState, Json, ValidationErrors};
use serde::{Deserialize, Serialize};

use super::{SettingField, SettingsMeta, SettingsPage};
use crate::context::Ctx;
use crate::http::{AdminCtx, json_body};

#[async_trait]
impl SettingsPage for MailSettings {
    fn meta() -> SettingsMeta {
        SettingsMeta::new("Mail").icon("mail")
    }

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::text("host").label("SMTP server").max_length(253),
            SettingField::integer("port").label("Port").required(),
            SettingField::select("encryption", [("starttls", "STARTTLS"), ("tls", "TLS"), ("none", "None")])
                .label("Encryption")
                .required(),
            SettingField::text("username").label("User name").max_length(255),
            SettingField::secret("password")
                .label("Password")
                .max_length(1_000)
                .kept_while(&["host", "port", "username"]),
            SettingField::text("from_address").label("Sender address").email().max_length(255),
            SettingField::text("from_name").label("Sender name").max_length(255),
            SettingField::text("reply_to").label("Reply-to address").email().max_length(255),
        ]
    }

    async fn validate_in(state: &AppState, _ctx: &Ctx, settings: &Self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        if settings.host.trim().is_empty() {
            return Ok(());
        }
        let policy = mail::mailer(state).map(|m| m.config().policy()).unwrap_or_default();
        if let Err(issue) =
            policy.check(&settings.host, settings.port, settings.encryption, !settings.username.is_empty())
        {
            errors.add(issue.field, issue.message);
        }
        if settings.from_address.trim().is_empty() {
            errors.add("from_address", "is required");
        }
        if settings.encryption == Encryption::None && settings.password.is_set() {
            errors.add("password", "is not allowed");
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestInput {
    to: String,
}

/// `POST settings/mail/test`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MailTestResult {
    /// Sent.
    pub ok: bool,
    /// Why not (`smtp_auth`, `mail_not_configured`, …).
    #[cfg_attr(feature = "ts", ts(optional))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Hides most of an address (`a***@example.com`) for the audit log.
fn masked(address: &str) -> String {
    match address.rsplit_once('@') {
        Some((user, domain)) => format!("{}***@{domain}", user.chars().next().unwrap_or('*')),
        None => "***".to_owned(),
    }
}

/// Longest wait for the test mail.
const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// `POST settings/mail/test` `{to}`: sends a test mail now with the saved
/// settings (never with values not yet saved). Allowed to whoever may update
/// the mail settings, checked before the body is read; five a minute.
pub(super) async fn test(
    State(state): State<AppState>,
    AdminCtx(ctx): AdminCtx,
    request: Request,
) -> AppResult<Json<MailTestResult>> {
    let settings = super::http::registry(&state)?;
    super::http::admit(&settings, &ctx.principal, "mail", true)?;
    let auth = state.service::<rustcms_auth::AuthState>().ok_or_else(|| AppError::internal("auth not registered"))?;
    auth.limiter.reserve(&[(&format!("mail-test:{}", ctx.principal.id), 5)]).await?;
    let TestInput { to } = json_body(request, &state).await?;
    let to = to.trim().to_owned();
    if !mail::valid_address(&to) {
        return Err(ValidationErrors::field("to", "must be a valid email address").into());
    }
    let outcome = async {
        let rendered = mail::render(&state, "rustcms.test", &ctx.locale, &serde_json::json!({}))?;
        mail::send(&state, &Message::rendered([to.clone()], rendered)).await
    };
    let outcome = tokio::time::timeout(TEST_TIMEOUT, outcome).await.unwrap_or(Err(mail::MailError::TIMEOUT));
    let code = outcome.err().map(|e| e.0.to_owned());
    let entry = rustcms_core::audit::AuditEntry::new("mail.test_sent", "settings")
        .meta(serde_json::json!({ "to": masked(&to), "ok": code.is_none(), "code": code }));
    rustcms_core::audit::record_detached(state.db(), &ctx.audit_context, entry).await;
    Ok(Json(MailTestResult { ok: code.is_none(), code }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn addresses_are_masked() {
        assert_eq!(super::masked("alice@example.com"), "a***@example.com");
        assert_eq!(super::masked("nope"), "***");
    }
}
