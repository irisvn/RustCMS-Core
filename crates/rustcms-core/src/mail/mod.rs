//! Mail (ADR 0022): messages rendered from templates and sent by the
//! configured driver (`[mail] driver`: `disabled`, `smtp`, `log`, `memory`),
//! SMTP read from the `mail` settings group at each send (or `[mail.smtp]`),
//! always within the operator's policy (hosts, ports, TLS), at most
//! `max_per_minute` per process.
//!
//! - [`render`] and [`send`]: now, in the request or job.
//! - [`send_later`]: a queued job dispatched in the transaction of the change
//!   (the mail exists only if the change commits). Its data stays in the
//!   `jobs` table: never put a secret in it (mails carrying a link with a
//!   secret are sent by a job of their own, which makes the secret itself).
//!
//! Errors are codes ([`MailError`]): no server reply, address, password or
//! body ever reaches a log, a job's error or a response.

mod policy;
mod template;
mod transport;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use policy::{Encryption, MailPolicy, PolicyIssue};
pub use template::{MailTemplate, MailTemplates, Rendered};
pub use transport::SentMail;

use crate::queue::{Job, JobCtx};
use crate::settings::{Secret, Settings};
use crate::{AppState, Environment};

/// Largest data a template is rendered with (serialized).
pub const MAX_DATA_BYTES: usize = 64 * 1024;
/// Most recipients of one message.
pub const MAX_RECIPIENTS: usize = 50;

/// How mail leaves the application.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Driver {
    /// No mail (sending answers `mail_disabled`).
    #[default]
    Disabled,
    /// SMTP.
    Smtp,
    /// Logged (recipient count, template, message id; the body only with
    /// `log_body` in local development).
    Log,
    /// Kept in memory (tests: `rustcms_testing::TestApp::mails`).
    Memory,
}

/// Where the SMTP server's settings come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The `mail` settings group ([`MailSettings`], edited in the panel), read at each send.
    #[default]
    Settings,
    /// `[mail.smtp]` of the configuration.
    Config,
}

/// `[mail.smtp]` (with `source = "config"`).
#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SmtpConfig {
    /// Server.
    pub host: String,
    /// Port (465: TLS, 587: STARTTLS).
    pub port: u16,
    /// `starttls` (default), `tls` or `none`.
    pub encryption: Encryption,
    /// User name (none: no authentication).
    pub username: String,
    /// Password (`RUSTCMS_MAIL__SMTP__PASSWORD`).
    pub password: Option<String>,
    /// Sender address.
    pub from_address: String,
    /// Sender name.
    pub from_name: String,
    /// Reply-to address.
    pub reply_to: String,
}

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("encryption", &self.encryption)
            .field("password", &self.password.as_ref().map(|_| "set"))
            .finish_non_exhaustive()
    }
}

/// `[mail]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MailConfig {
    /// See [`Driver`].
    pub driver: Driver,
    /// See [`Source`].
    pub source: Source,
    /// SMTP server with `source = "config"`.
    pub smtp: SmtpConfig,
    /// Servers mail may be sent to: names (`smtp.example.com`), `*.example.com`,
    /// or `*` (any). Required in production when the panel sets the server.
    pub allowed_hosts: Vec<String>,
    /// Ports mail may be sent to.
    pub allowed_ports: Vec<u16>,
    /// Allows `encryption = "none"` (no authentication then).
    pub allow_insecure: bool,
    /// Longest send, connection included.
    pub timeout_secs: u64,
    /// Mails one process sends per minute (each web and worker process counts alone).
    pub max_per_minute: u32,
    /// Log driver: also log recipients, subject and body (local only).
    pub log_body: bool,
    /// Extra root certificate (PEM) trusted for SMTP TLS (a test server).
    pub tls_ca_file: Option<std::path::PathBuf>,
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            driver: Driver::Disabled,
            source: Source::Settings,
            smtp: SmtpConfig::default(),
            allowed_hosts: Vec::new(),
            allowed_ports: vec![465, 587],
            allow_insecure: false,
            timeout_secs: 30,
            max_per_minute: 60,
            log_body: false,
            tls_ca_file: None,
        }
    }
}

impl MailConfig {
    /// `[mail]` of `config`, checked.
    pub fn of(config: &crate::Config) -> anyhow::Result<Self> {
        let mail: Self = config.section("mail")?;
        mail.validated(config.app.env)
    }

    fn validated(self, env: Environment) -> anyhow::Result<Self> {
        anyhow::ensure!((1..=300).contains(&self.timeout_secs), "mail.timeout_secs must be between 1 and 300");
        anyhow::ensure!((1..=10_000).contains(&self.max_per_minute), "mail.max_per_minute must be between 1 and 10000");
        anyhow::ensure!(
            !self.log_body || env == Environment::Local,
            "mail.log_body prints mail bodies (reset links): only with APP_ENV=local"
        );
        if self.driver == Driver::Smtp {
            anyhow::ensure!(
                !(env == Environment::Production && self.source == Source::Settings && self.allowed_hosts.is_empty()),
                "mail.allowed_hosts is required in production when the panel sets the SMTP server \
                 (list the servers, or `*` to trust whoever may edit the mail settings)"
            );
            if self.source == Source::Config {
                self.policy()
                    .check(&self.smtp.host, self.smtp.port, self.smtp.encryption, !self.smtp.username.is_empty())
                    .map_err(|issue| anyhow::anyhow!("mail.smtp.{}: {}", issue.field, issue.message))?;
            }
        }
        Ok(self)
    }

    /// The operator's policy for SMTP servers.
    pub fn policy(&self) -> MailPolicy {
        MailPolicy {
            allowed_hosts: self.allowed_hosts.clone(),
            allowed_ports: self.allowed_ports.clone(),
            allow_insecure: self.allow_insecure,
        }
    }
}

/// The `mail` settings group: the SMTP server, edited in the panel
/// (`AdminProvider::mail_settings`). Read at each send.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MailSettings {
    /// Server.
    pub host: String,
    /// Port.
    pub port: u16,
    /// `starttls`, `tls` or `none`.
    pub encryption: Encryption,
    /// User name (empty: no authentication).
    pub username: String,
    /// Password (kept only while host, port and user name stay the same).
    pub password: Secret,
    /// Sender address.
    pub from_address: String,
    /// Sender name.
    pub from_name: String,
    /// Reply-to address.
    pub reply_to: String,
}

impl Default for MailSettings {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 587,
            encryption: Encryption::Starttls,
            username: String::new(),
            password: Secret::default(),
            from_address: String::new(),
            from_name: String::new(),
            reply_to: String::new(),
        }
    }
}

impl Settings for MailSettings {
    const GROUP: &'static str = "mail";
}

/// Why mail was not sent: a stable code, safe to log and show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailError(pub &'static str);

impl MailError {
    /// Mail is off (`driver = "disabled"`).
    pub const DISABLED: Self = Self("mail_disabled");
    /// No SMTP server set yet.
    pub const NOT_CONFIGURED: Self = Self("mail_not_configured");
    /// The server is outside the operator's policy.
    pub const POLICY: Self = Self("mail_policy");
    /// The saved password cannot be decrypted (`APP_KEY` changed).
    pub const SETTINGS_UNREADABLE: Self = Self("mail_settings_unreadable");
    /// An address or header is invalid.
    pub const INVALID: Self = Self("mail_invalid");
    /// A template is missing or failed.
    pub const TEMPLATE: Self = Self("mail_template");
    /// Could not connect.
    pub const CONNECT: Self = Self("smtp_connect");
    /// TLS failed (or the server offers none).
    pub const TLS: Self = Self("smtp_tls");
    /// Authentication refused.
    pub const AUTH: Self = Self("smtp_auth");
    /// The server refused the message (5xx).
    pub const REJECTED: Self = Self("smtp_rejected");
    /// The server deferred it (4xx).
    pub const TEMPORARY: Self = Self("smtp_temporary");
    /// Too slow.
    pub const TIMEOUT: Self = Self("smtp_timeout");
    /// This process's send rate was reached.
    pub const THROTTLED: Self = Self("smtp_throttled");

    /// Retrying cannot help (until someone changes something).
    pub fn is_permanent(self) -> bool {
        matches!(
            self.0,
            "mail_disabled"
                | "mail_not_configured"
                | "mail_policy"
                | "mail_settings_unreadable"
                | "mail_invalid"
                | "mail_template"
                | "smtp_rejected"
                | "smtp_auth"
        )
    }

    /// A short message (English, no details).
    pub fn message(self) -> &'static str {
        match self.0 {
            "mail_disabled" => "Mail is disabled.",
            "mail_not_configured" => "No mail server is set.",
            "mail_policy" => "The mail server is not allowed.",
            "mail_settings_unreadable" => "The mail password cannot be decrypted; set it again.",
            "mail_invalid" => "An address or header is invalid.",
            "mail_template" => "The mail template failed.",
            "smtp_connect" => "Could not connect to the mail server.",
            "smtp_tls" => "Could not secure the connection to the mail server.",
            "smtp_auth" => "The mail server refused the user name or password.",
            "smtp_rejected" => "The mail server refused the message.",
            "smtp_temporary" => "The mail server deferred the message.",
            "smtp_timeout" => "The mail server did not answer in time.",
            _ => "Too many mails at once; retried later.",
        }
    }
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for MailError {}

impl From<MailError> for crate::queue::Failure {
    fn from(err: MailError) -> Self {
        crate::queue::Failure::new(err.0, err.message())
    }
}

/// A message to send.
#[derive(Debug, Clone, Default)]
pub struct Message {
    /// Recipients (addresses, `Name <address>` allowed).
    pub to: Vec<String>,
    /// Subject (one line).
    pub subject: String,
    /// Plain text body.
    pub text: String,
    /// HTML body (sent as an alternative to the text).
    pub html: Option<String>,
    /// `Message-ID` (without `<>`; generated when absent).
    pub message_id: Option<String>,
}

impl Message {
    /// A message to `to` from a rendered template.
    pub fn rendered(to: impl IntoIterator<Item = String>, rendered: Rendered) -> Self {
        Self {
            to: to.into_iter().collect(),
            subject: rendered.subject,
            text: rendered.text,
            html: rendered.html,
            message_id: None,
        }
    }
}

/// The bound mailer (see [`MailProvider`]).
pub struct Mailer {
    config: MailConfig,
    transport: transport::Transport,
    limiter: transport::Limiter,
}

impl Mailer {
    /// A mailer for `config`.
    pub fn new(config: MailConfig) -> anyhow::Result<Self> {
        let transport = transport::Transport::new(&config)?;
        let limiter = transport::Limiter::new(config.max_per_minute);
        Ok(Self { config, transport, limiter })
    }

    /// The configuration.
    pub fn config(&self) -> &MailConfig {
        &self.config
    }

    /// Mails kept by the `memory` driver (oldest first).
    pub fn sent(&self) -> Vec<SentMail> {
        self.transport.sent()
    }

    /// Takes the mails kept by the `memory` driver.
    pub fn take_sent(&self) -> Vec<SentMail> {
        self.transport.take_sent()
    }
}

/// The bound [`Mailer`] of `state`.
pub fn mailer(state: &AppState) -> Option<Arc<Mailer>> {
    state.service::<Mailer>()
}

/// Renders template `name` in `locale` (else `app.locale`) with `data`
/// (`app_name`, `app_url` and `locale` are added).
pub fn render(state: &AppState, name: &str, locale: &str, data: &Value) -> Result<Rendered, MailError> {
    let templates = state.service::<MailTemplates>().ok_or(MailError::TEMPLATE)?;
    templates.render(state.config(), name, locale, data)
}

/// Sends `message` now (`Message-ID` from `message.message_id`).
pub async fn send(state: &AppState, message: &Message) -> Result<(), MailError> {
    let mailer = mailer(state).ok_or(MailError::DISABLED)?;
    mailer.transport.send(state, &mailer.config, &mailer.limiter, message).await
}

/// A mail sent by the queue ([`send_later`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailJob {
    /// Recipients.
    pub to: Vec<String>,
    /// Template name.
    pub template: String,
    /// Language of the template.
    pub locale: String,
    /// Data of the template (no secrets: it stays in the `jobs` table).
    pub data: Value,
}

/// Queues `mail` in `txn` (sent once the change commits; retried on
/// temporary failures, failed at once on permanent ones).
pub async fn send_later(txn: &sea_orm::DatabaseTransaction, mail: MailJob) -> crate::AppResult<()> {
    if mail.to.is_empty() || mail.to.len() > MAX_RECIPIENTS {
        return Err(crate::AppError::internal("mail: 1 to 50 recipients"));
    }
    crate::queue::dispatch(txn, &mail).await?;
    Ok(())
}

#[async_trait]
impl Job for MailJob {
    const KIND: &'static str = "mail.send";

    fn max_attempts() -> u32 {
        8
    }

    fn timeout() -> Duration {
        Duration::from_secs(120)
    }

    async fn handle(self, job: JobCtx) -> anyhow::Result<()> {
        let outcome = async {
            let rendered = render(&job.state, &self.template, &self.locale, &self.data)?;
            let mut message = Message::rendered(self.to.clone(), rendered);
            // Recognizable when a retry repeats a mail the server took.
            message.message_id = Some(transport::message_id(&job.state, job.id, job.attempt));
            send(&job.state, &message).await
        }
        .await;
        match outcome {
            Ok(()) => Ok(()),
            Err(err) if err.is_permanent() => Err(crate::queue::Failure::from(err).permanent().into()),
            Err(err) => Err(crate::queue::Failure::from(err).into()),
        }
    }
}

/// Binds the [`Mailer`] for `[mail]` and the `mail.send` job. Boot fails on
/// a misconfiguration (an SMTP server outside the policy, `log_body` outside
/// local development, no allowed host in production).
pub struct MailProvider;

impl crate::ServiceProvider for MailProvider {
    fn register(&self, app: &mut crate::AppBuilder) {
        let config = MailConfig::of(app.config()).unwrap_or_else(|e| panic!("invalid [mail] configuration: {e:#}"));
        let mailer = Mailer::new(config).unwrap_or_else(|e| panic!("mail unavailable: {e:#}"));
        app.bind(mailer);
        app.job::<MailJob>();
        app.mail_template(crate::mail_template!("rustcms.test", "en"));
        app.mail_template(crate::mail_template!("rustcms.test", "vi"));
    }
}

/// Whether `text` is one email address (`user@domain`, no name, no control characters).
pub fn valid_address(text: &str) -> bool {
    !text.chars().any(char::is_control) && text.len() <= 254 && text.parse::<lettre::Address>().is_ok()
}
