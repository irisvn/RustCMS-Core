//! The jobs that make and mail tokens. Their payloads carry no secret: the
//! token is made when the job runs, under the user's lock, and mailed at once.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use rustcms_core::AppState;
use rustcms_core::mail::{self, MailError, Message};
use rustcms_core::models::user;
use rustcms_core::queue::{Failure, Job, JobCtx};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Issued, Purpose, Request, issue, link, mark_sent};
use crate::state::AuthState;

/// A reset request older than this sends nothing (the user asked long ago).
const RESET_REQUEST_TTL: Duration = Duration::minutes(15);

/// `auth.password_reset`: mails a reset link to the account of `email`, if
/// there is one that may reset its password (nothing otherwise: the request
/// answered the same either way).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordResetJob {
    /// As typed (lower case).
    pub email: String,
    /// Language of the request (the user's own choice wins).
    pub locale: String,
    /// The request.
    pub request_id: uuid::Uuid,
    /// When it was made.
    pub requested_at: DateTime<Utc>,
}

/// `auth.invitation`: mails an invitation link to a pending user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationJob {
    /// The invited user.
    pub user_id: i64,
    /// Their credential version when invited (a change cancels the mail).
    pub auth_version: i64,
    /// The request (an invitation, or sending it again).
    pub request_id: uuid::Uuid,
    /// When it was made.
    pub requested_at: DateTime<Utc>,
}

fn failure(err: MailError) -> anyhow::Error {
    if err.is_permanent() { Failure::from(err).permanent().into() } else { Failure::from(err).into() }
}

fn auth(state: &AppState) -> anyhow::Result<std::sync::Arc<AuthState>> {
    state.service::<AuthState>().ok_or_else(|| anyhow::anyhow!("AuthProvider not registered"))
}

/// Renders and sends the mail of `issued`, then records it sent.
async fn deliver(
    state: &AppState,
    auth: &AuthState,
    purpose: Purpose,
    issued: &Issued,
    locale: &str,
    valid_for: &str,
) -> anyhow::Result<()> {
    let config = state.config();
    let url = link(&config.app.url, &auth.config.admin_path, purpose, &issued.token);
    let data = json!({ "name": issued.user.name, "link": url, "valid_for": valid_for });
    let rendered = mail::render(state, purpose.template(), locale, &data).map_err(failure)?;
    mail::send(state, &Message::rendered([issued.user.email.clone()], rendered)).await.map_err(failure)?;
    // Best effort: a run cut here sends a new link on its retry (the first one then fails).
    if let Err(err) = mark_sent(state.db(), issued).await {
        tracing::warn!(%err, "token mail sent; not recorded");
    }
    Ok(())
}

fn locale_of(state: &AppState, user: &user::Model, asked: &str) -> String {
    let offered = &state.config().app.locales;
    user.locale.clone().filter(|l| offered.contains(l)).unwrap_or_else(|| {
        if offered.iter().any(|l| l == asked) { asked.to_owned() } else { state.config().app.locale.clone() }
    })
}

#[async_trait]
impl Job for PasswordResetJob {
    const KIND: &'static str = "auth.password_reset";

    fn max_attempts() -> u32 {
        5
    }

    fn timeout() -> std::time::Duration {
        std::time::Duration::from_secs(120)
    }

    async fn handle(self, job: JobCtx) -> anyhow::Result<()> {
        let auth = auth(&job.state)?;
        if !auth.config.password_reset {
            return Ok(());
        }
        if Utc::now() > self.requested_at + RESET_REQUEST_TTL {
            return Err(Failure::new("expired_before_sent", "The request is too old to answer.").permanent().into());
        }
        let Some(user) = user::Entity::find().filter(user::Column::Email.eq(&self.email)).one(job.state.db()).await?
        else {
            return Ok(());
        };
        let minutes = i64::try_from(auth.config.password_reset_minutes).unwrap_or(60);
        let request = Request { id: self.request_id, at: self.requested_at };
        let cooldown = Duration::seconds(i64::try_from(auth.config.reset_cooldown_secs).unwrap_or(60));
        let email = self.email.clone();
        let issued = issue(
            job.state.db(),
            &job.state.config().app.key,
            Purpose::PasswordReset,
            user.id,
            // The same account as asked for, locked (an email changed meanwhile: nothing).
            move |u| u.email == email,
            request,
            Duration::minutes(minutes),
            Some(cooldown),
        )
        .await?;
        let Some(issued) = issued else { return Ok(()) };
        let locale = locale_of(&job.state, &issued.user, &self.locale);
        deliver(&job.state, &auth, Purpose::PasswordReset, &issued, &locale, &minutes.to_string()).await
    }
}

#[async_trait]
impl Job for InvitationJob {
    const KIND: &'static str = "auth.invitation";

    fn max_attempts() -> u32 {
        8
    }

    fn timeout() -> std::time::Duration {
        std::time::Duration::from_secs(120)
    }

    async fn handle(self, job: JobCtx) -> anyhow::Result<()> {
        let auth = auth(&job.state)?;
        let days = i64::try_from(auth.config.invitation_days).unwrap_or(7);
        if Utc::now() > self.requested_at + Duration::days(days) {
            return Err(Failure::new("expired_before_sent", "The invitation is too old to send.").permanent().into());
        }
        let request = Request { id: self.request_id, at: self.requested_at };
        let version = self.auth_version;
        let issued = issue(
            job.state.db(),
            &job.state.config().app.key,
            Purpose::Invitation,
            self.user_id,
            move |u| u.auth_version == version,
            request,
            Duration::days(days),
            None,
        )
        .await?;
        let Some(issued) = issued else { return Ok(()) };
        let locale = locale_of(&job.state, &issued.user, &job.state.config().app.locale);
        deliver(&job.state, &auth, Purpose::Invitation, &issued, &locale, &days.to_string()).await
    }
}
