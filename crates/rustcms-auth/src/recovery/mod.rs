//! Password resets and invitations (ADR 0023).
//!
//! A token is `{id}.{secret}`: the id names a `user_tokens` row, the secret
//! (32 random bytes, base64url) is only in the mail; the row keeps an HMAC of
//! purpose, id and secret under a key derived from `APP_KEY`. At most one
//! token lives per user and purpose.
//!
//! Tokens are made only by the jobs that mail them, under the user's row lock
//! (never in a queue payload): a job carries the request's id and time, so a
//! retry re-issues for its own request, an older request never replaces a
//! newer one, and a request older than its deadline sends nothing. A token
//! is used under the same lock order (user, then token), checked again in
//! full, and the password, the deletion of every token of the user and the
//! revocation of their credentials commit together.

mod jobs;

use chrono::{DateTime, Duration, Utc};
use rustcms_core::audit::{AuditActor, AuditContext, AuditEntry, RequestMeta};
use rustcms_core::models::user;
use rustcms_core::{AppError, AppKey, AppResult, ValidationErrors};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, QueryFilter,
    QuerySelect, Set,
};

pub use jobs::{InvitationJob, PasswordResetJob};

use crate::entities::user_token as token;
use crate::secret::{ct_eq, hmac_sha256_hex};

/// Password of an invited user until they accept: not a password hash, so
/// nothing ever matches it (`Passwords::verify` takes it as an unknown user).
pub const INVITED_PASSWORD: &str = "!invited";
const KEY_PURPOSE: &str = "rustcms/user-tokens/v1";
/// Characters of a secret (32 bytes, base64url without padding).
const SECRET_LEN: usize = 43;
/// Longest token accepted (uuid, dot, secret).
const MAX_TOKEN_LEN: usize = 80;
/// How long the mark left by a credential change is kept: longer than any
/// request may wait (`invitation_days` is at most 30).
const TOMBSTONE_DAYS: i64 = 30;
/// Most days an invitation link may live.
pub const MAX_INVITATION_DAYS: u64 = 30;

/// What a token is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// "Forgot password?".
    PasswordReset,
    /// An administrator's invitation.
    Invitation,
}

impl Purpose {
    /// Stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PasswordReset => "password_reset",
            Self::Invitation => "invitation",
        }
    }

    /// The panel's page for the link.
    fn page(self) -> &'static str {
        match self {
            Self::PasswordReset => "reset-password",
            Self::Invitation => "invitation",
        }
    }

    /// The mail template.
    fn template(self) -> &'static str {
        match self {
            Self::PasswordReset => "auth.password_reset",
            Self::Invitation => "auth.invitation",
        }
    }

    /// Whether `user` may use a token of this purpose now.
    fn admits(self, user: &user::Model) -> bool {
        user.is_active
            && match self {
                Self::PasswordReset => !user.invitation_pending,
                Self::Invitation => user.invitation_pending,
            }
    }
}

/// The one answer for a token that cannot be used (unknown, wrong, expired,
/// used, superseded, or its account changed): nothing more is told.
pub(crate) fn invalid() -> AppError {
    ValidationErrors::field("token", "is invalid or has expired").into()
}

fn hash(key: &AppKey, purpose: Purpose, id: uuid::Uuid, secret: &str) -> String {
    hmac_sha256_hex(&key.derive(KEY_PURPOSE), &format!("{}\n{id}\n{secret}", purpose.as_str()))
}

/// The id and secret of a well-formed token (`{uuid}.{43 base64url}`).
pub(crate) fn parse(text: &str) -> Option<(uuid::Uuid, &str)> {
    if text.len() > MAX_TOKEN_LEN {
        return None;
    }
    let (id, secret) = text.split_once('.')?;
    let parsed = uuid::Uuid::parse_str(id).ok()?;
    // Canonical forms only (one token, one spelling).
    let canonical = parsed.hyphenated().to_string() == id;
    let secret_ok =
        secret.len() == SECRET_LEN && secret.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    (canonical && secret_ok).then_some((parsed, secret))
}

fn new_secret() -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

/// A request for a token (a queued job's).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Request {
    pub(crate) id: uuid::Uuid,
    pub(crate) at: DateTime<Utc>,
}

/// A token made for a request, to mail now.
pub(crate) struct Issued {
    pub(crate) token: String,
    pub(crate) id: uuid::Uuid,
    pub(crate) hash: String,
    pub(crate) user: user::Model,
}

/// Makes the token of `purpose` for `user_id` answering `request`, under the
/// user's lock: `None` when nothing is to be sent (the user no longer
/// qualifies, `still` refuses them, the request was answered or superseded,
/// or a token for another request was made less than `cooldown` ago).
#[allow(clippy::too_many_arguments)] // one request's whole description
pub(crate) async fn issue(
    db: &DatabaseConnection,
    key: &AppKey,
    purpose: Purpose,
    user_id: i64,
    still: impl Fn(&user::Model) -> bool,
    request: Request,
    ttl: Duration,
    cooldown: Option<Duration>,
) -> AppResult<Option<Issued>> {
    let txn = rustcms_core::db::begin_write(db).await?;
    let Some(user) = user::Entity::find_by_id(user_id).lock_exclusive().one(&txn).await? else {
        return Ok(None);
    };
    if !purpose.admits(&user) || !still(&user) {
        return Ok(None);
    }
    let mut find = token::Entity::find()
        .filter(token::Column::UserId.eq(user_id))
        .filter(token::Column::Purpose.eq(purpose.as_str()));
    if txn.get_database_backend() != DbBackend::Sqlite {
        find = find.lock_exclusive();
    }
    let now = Utc::now();
    if let Some(row) = find.one(&txn).await? {
        let answered = row.request_id == request.id && row.sent_at.is_some();
        let superseded = row.request_id != request.id && row.requested_at > request.at;
        // The mark of a credential change is not a token: it never delays a new request.
        let recent = row.request_id != request.id
            && !row.token_hash.is_empty()
            && cooldown.is_some_and(|c| row.issued_at > now - c);
        if answered || superseded || recent {
            return Ok(None);
        }
        token::Entity::delete_by_id(row.id).exec(&txn).await?;
    }
    let id = uuid::Uuid::now_v7();
    let secret = new_secret();
    let hash = hash(key, purpose, id, &secret);
    token::ActiveModel {
        id: Set(id),
        user_id: Set(user.id),
        purpose: Set(purpose.as_str().to_owned()),
        email: Set(user.email.clone()),
        auth_version: Set(user.auth_version),
        token_hash: Set(hash.clone()),
        request_id: Set(request.id),
        requested_at: Set(request.at),
        issued_at: Set(now),
        expires_at: Set(now + ttl),
        sent_at: Set(None),
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;
    Ok(Some(Issued { token: format!("{id}.{secret}"), id, hash, user }))
}

/// Records that the mail of `issued` was sent (only if it is still that token).
pub(crate) async fn mark_sent(db: &DatabaseConnection, issued: &Issued) -> AppResult<()> {
    token::Entity::update_many()
        .col_expr(token::Column::SentAt, Expr::value(Utc::now()))
        .filter(token::Column::Id.eq(issued.id))
        .filter(token::Column::TokenHash.eq(&issued.hash))
        .exec(db)
        .await?;
    Ok(())
}

/// The link of `token` (the secret in the fragment: never in a server log
/// nor a `Referer`).
pub(crate) fn link(app_url: &str, admin_path: &str, purpose: Purpose, token: &str) -> String {
    format!("{}{}/{}#token={token}", app_url.trim_end_matches('/'), admin_path.trim_end_matches('/'), purpose.page())
}

/// The row of a well-formed token whose secret matches, unexpired, of
/// `purpose` (read without a lock: a cheap check before hashing a password).
pub(crate) async fn check(
    db: &DatabaseConnection,
    key: &AppKey,
    purpose: Purpose,
    text: &str,
) -> AppResult<token::Model> {
    let (id, secret) = parse(text).ok_or_else(invalid)?;
    let row = token::Entity::find_by_id(id).one(db).await?.ok_or_else(invalid)?;
    let matches = ct_eq(&row.token_hash, &hash(key, purpose, id, secret));
    if !matches || row.purpose != purpose.as_str() || row.expires_at <= Utc::now() {
        return Err(invalid());
    }
    Ok(row)
}

/// What using a token changes besides the password.
pub(crate) struct Accept {
    /// The user's new name (invitations).
    pub(crate) name: Option<String>,
    /// Revoke API tokens too.
    pub(crate) revoke_api_tokens: bool,
}

/// Uses token `text` of `purpose` (checked before by [`check`]) to set
/// `password_hash`: under the user's lock then the token's, everything is
/// checked again; the password, the end of every token of the user and the
/// revocation of their sessions and tokens commit together.
#[allow(clippy::too_many_arguments)] // the token, its row, the new password and the request
pub(crate) async fn consume(
    db: &DatabaseConnection,
    key: &AppKey,
    purpose: Purpose,
    text: &str,
    checked: &token::Model,
    password_hash: String,
    accept: Accept,
    request: RequestMeta,
) -> AppResult<user::Model> {
    let (id, secret) = parse(text).ok_or_else(invalid)?;
    let txn = rustcms_core::db::begin_write(db).await?;
    let user = user::Entity::find_by_id(checked.user_id).lock_exclusive().one(&txn).await?.ok_or_else(invalid)?;
    let mut find = token::Entity::find_by_id(id);
    if txn.get_database_backend() != DbBackend::Sqlite {
        find = find.lock_exclusive();
    }
    let row = find.one(&txn).await?.ok_or_else(invalid)?;
    let valid = ct_eq(&row.token_hash, &hash(key, purpose, id, secret))
        && row.purpose == purpose.as_str()
        && row.user_id == user.id
        && row.expires_at > Utc::now()
        && row.email == user.email
        && row.auth_version == user.auth_version
        && purpose.admits(&user);
    if !valid {
        return Err(invalid());
    }
    let mut model: user::ActiveModel = user.clone().into();
    model.password = Set(password_hash);
    model.updated_at = Set(Utc::now());
    if purpose == Purpose::Invitation {
        model.invitation_pending = Set(false);
        if user.email_verified_at.is_none() {
            // The mail reached them: the address is theirs.
            model.email_verified_at = Set(Some(Utc::now()));
        }
        if let Some(name) = accept.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            model.name = Set(name.to_owned());
        }
    }
    let user = model.update(&txn).await?;
    end_tokens(&txn, &user).await?;
    let version = crate::credentials::revoke_all(&txn, user.id, accept.revoke_api_tokens).await?;
    let event = match purpose {
        Purpose::PasswordReset => "auth.password_reset",
        Purpose::Invitation => "auth.invitation_accepted",
    };
    let context = AuditContext::new(AuditActor::signed_in(user.id, crate::ADMIN_GUARD, None), request);
    rustcms_core::audit::record(&txn, &context, AuditEntry::new(event, "user").key(user.id)).await?;
    txn.commit().await?;
    Ok(user::Model { auth_version: version, ..user })
}

/// Ends the tokens of `user` (in the caller's transaction: a token used, a
/// password set by an administrator), leaving per purpose a mark dated now
/// that no secret matches: a request made before it (a retried older reset,
/// a delayed invitation job) counts as superseded and sends nothing.
pub async fn end_tokens<C: sea_orm::ConnectionTrait>(conn: &C, user: &user::Model) -> AppResult<()> {
    token::Entity::delete_many().filter(token::Column::UserId.eq(user.id)).exec(conn).await?;
    let now = Utc::now();
    for purpose in [Purpose::PasswordReset, Purpose::Invitation] {
        token::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            user_id: Set(user.id),
            purpose: Set(purpose.as_str().to_owned()),
            email: Set(user.email.clone()),
            auth_version: Set(user.auth_version),
            // Not a hash: nothing verifies against it.
            token_hash: Set(String::new()),
            request_id: Set(uuid::Uuid::nil()),
            requested_at: Set(now),
            issued_at: Set(now),
            expires_at: Set(now + Duration::days(TOMBSTONE_DAYS)),
            sent_at: Set(Some(now)),
        }
        .insert(conn)
        .await?;
    }
    Ok(())
}

/// Deletes expired tokens, in batches; returns how many.
pub async fn prune(db: &DatabaseConnection) -> AppResult<u64> {
    let mut total = 0;
    loop {
        let expired: Vec<uuid::Uuid> = token::Entity::find()
            .select_only()
            .column(token::Column::Id)
            .filter(token::Column::ExpiresAt.lte(Utc::now()))
            .limit(500)
            .into_tuple()
            .all(db)
            .await?;
        if expired.is_empty() {
            return Ok(total);
        }
        let deleted = token::Entity::delete_many().filter(token::Column::Id.is_in(expired)).exec(db).await?;
        total += deleted.rows_affected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_have_one_spelling() {
        let id = uuid::Uuid::now_v7();
        let secret = new_secret();
        assert_eq!(secret.len(), SECRET_LEN);
        let token = format!("{id}.{secret}");
        assert_eq!(parse(&token), Some((id, secret.as_str())));
        for bad in [
            String::new(),
            format!("{id}"),
            format!("{}.{secret}", id.simple()),
            format!("{}.{secret}", id.to_string().to_uppercase()),
            format!("{id}.{}", &secret[1..]),
            format!("{id}.{secret}x"),
            format!("{id}.{}+", &secret[1..]),
            format!("{id}.{secret}.x"),
        ] {
            assert!(parse(&bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn hashes_bind_purpose_id_and_key() {
        let (key, other) = (AppKey::generate(), AppKey::generate());
        let id = uuid::Uuid::now_v7();
        let base = hash(&key, Purpose::PasswordReset, id, "s");
        assert_ne!(base, hash(&key, Purpose::Invitation, id, "s"));
        assert_ne!(base, hash(&key, Purpose::PasswordReset, uuid::Uuid::now_v7(), "s"));
        assert_ne!(base, hash(&other, Purpose::PasswordReset, id, "s"));
        assert_eq!(base, hash(&key, Purpose::PasswordReset, id, "s"));
    }
}
