//! Drivers: SMTP (lettre, rustls), log, memory; the per-process rate limit;
//! lettre errors mapped to codes (the server's text is never kept).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;

use lettre::message::{Mailbox, MultiPart, SinglePart, header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Certificate, Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use super::policy::{Encryption, valid_host};
use super::{Driver, MAX_RECIPIENTS, MailConfig, MailError, MailSettings, Message, Source};
use crate::AppState;

/// A mail kept by the `memory` driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMail {
    /// Recipients.
    pub to: Vec<String>,
    /// Sender.
    pub from: String,
    /// Subject.
    pub subject: String,
    /// Text body.
    pub text: String,
    /// HTML body.
    pub html: Option<String>,
    /// `Message-ID`.
    pub message_id: String,
}

pub(super) enum Transport {
    Disabled,
    Smtp { roots: Vec<Certificate> },
    Log { body: bool },
    Memory(Arc<Mutex<Vec<SentMail>>>),
}

impl Transport {
    pub(super) fn new(config: &MailConfig) -> anyhow::Result<Self> {
        Ok(match config.driver {
            Driver::Disabled => Self::Disabled,
            Driver::Log => Self::Log { body: config.log_body },
            Driver::Memory => Self::Memory(Arc::default()),
            Driver::Smtp => {
                let mut roots = Vec::new();
                if let Some(path) = &config.tls_ca_file {
                    let pem =
                        std::fs::read(path).map_err(|e| anyhow::anyhow!("mail.tls_ca_file {}: {e}", path.display()))?;
                    roots.push(
                        Certificate::from_pem(&pem)
                            .map_err(|_| anyhow::anyhow!("mail.tls_ca_file: not a PEM certificate"))?,
                    );
                }
                Self::Smtp { roots }
            }
        })
    }

    pub(super) fn sent(&self) -> Vec<SentMail> {
        match self {
            Self::Memory(sent) => sent.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone(),
            _ => Vec::new(),
        }
    }

    pub(super) fn take_sent(&self) -> Vec<SentMail> {
        match self {
            Self::Memory(sent) => std::mem::take(&mut *sent.lock().unwrap_or_else(std::sync::PoisonError::into_inner)),
            _ => Vec::new(),
        }
    }

    pub(super) async fn send(
        &self,
        state: &AppState,
        config: &MailConfig,
        limiter: &Limiter,
        message: &Message,
    ) -> Result<(), MailError> {
        if matches!(self, Self::Disabled) {
            return Err(MailError::DISABLED);
        }
        let server = Server::resolve(state, config, matches!(self, Self::Smtp { .. })).await?;
        let (email, envelope) = build(state, &server, message)?;
        limiter.acquire().await?;
        match self {
            Self::Disabled => Err(MailError::DISABLED),
            Self::Log { body } => {
                tracing::info!(
                    target: "rustcms::mail",
                    recipients = envelope.to.len(),
                    message_id = %envelope.message_id,
                    "mail (log driver)"
                );
                if *body {
                    tracing::info!(target: "rustcms::mail", to = ?envelope.to, subject = %message.subject, text = %message.text, "mail body (log_body)");
                }
                Ok(())
            }
            Self::Memory(sent) => {
                sent.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(SentMail {
                    to: envelope.to,
                    from: server.from.to_string(),
                    subject: message.subject.clone(),
                    text: message.text.clone(),
                    html: message.html.clone(),
                    message_id: envelope.message_id,
                });
                Ok(())
            }
            Self::Smtp { roots } => {
                let transport = smtp(state, config, &server, roots)?;
                let timeout = Duration::from_secs(config.timeout_secs);
                match tokio::time::timeout(timeout, transport.send(email)).await {
                    Err(_) => Err(MailError::TIMEOUT),
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(err)) => {
                        let code = classify(&err, server.encryption);
                        // The server's reply may quote the message or the user: its code only.
                        tracing::warn!(
                            target: "rustcms::mail",
                            code = code.0,
                            status = ?err.status().map(|c| c.to_string()),
                            "mail not sent"
                        );
                        Err(code)
                    }
                }
            }
        }
    }
}

/// The server and sender, read now (settings) or from the configuration.
struct Server {
    host: String,
    port: u16,
    encryption: Encryption,
    username: String,
    password: Option<String>,
    from: Mailbox,
    reply_to: Option<Mailbox>,
}

fn mailbox(address: &str, name: &str) -> Result<Mailbox, MailError> {
    if name.chars().any(char::is_control) || address.chars().any(char::is_control) {
        return Err(MailError::INVALID);
    }
    let address = address.trim().parse().map_err(|_| MailError::INVALID)?;
    Ok(Mailbox::new((!name.trim().is_empty()).then(|| name.trim().to_owned()), address))
}

impl Server {
    async fn resolve(state: &AppState, config: &MailConfig, smtp: bool) -> Result<Self, MailError> {
        let settings = match config.source {
            Source::Config => {
                let s = &config.smtp;
                MailSettings {
                    host: s.host.clone(),
                    port: s.port,
                    encryption: s.encryption,
                    username: s.username.clone(),
                    password: s.password.clone().map(crate::settings::Secret::new).unwrap_or_default(),
                    from_address: s.from_address.clone(),
                    from_name: s.from_name.clone(),
                    reply_to: s.reply_to.clone(),
                }
            }
            Source::Settings => crate::settings::get::<MailSettings>(state).await.map_err(|_| {
                // Unreadable: the password no longer decrypts (or the row is damaged).
                MailError::SETTINGS_UNREADABLE
            })?,
        };
        if settings.from_address.trim().is_empty() || (smtp && settings.host.trim().is_empty()) {
            return Err(MailError::NOT_CONFIGURED);
        }
        if smtp {
            config
                .policy()
                .check(&settings.host, settings.port, settings.encryption, !settings.username.is_empty())
                .map_err(|_| MailError::POLICY)?;
        }
        let reply_to = settings.reply_to.trim();
        Ok(Self {
            host: settings.host.trim().to_ascii_lowercase(),
            port: settings.port,
            encryption: settings.encryption,
            username: settings.username.clone(),
            password: settings.password.expose().map(str::to_owned),
            from: mailbox(&settings.from_address, &settings.from_name)?,
            reply_to: (!reply_to.is_empty()).then(|| mailbox(reply_to, "")).transpose()?,
        })
    }
}

struct Envelope {
    to: Vec<String>,
    message_id: String,
}

/// The host part of `APP_URL` (for `Message-ID` and `EHLO`), `localhost` otherwise.
fn app_host(state: &AppState) -> String {
    let url = &state.config().app.url;
    let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
    let host = rest.split(['/', ':', '?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    if valid_host(&host) { host } else { "localhost".to_owned() }
}

/// `Message-ID` of run `attempt` of job `id`.
pub(super) fn message_id(state: &AppState, id: uuid::Uuid, attempt: u32) -> String {
    format!("{id}.{attempt}@{}", app_host(state))
}

fn build(state: &AppState, server: &Server, message: &Message) -> Result<(lettre::Message, Envelope), MailError> {
    if message.to.is_empty() || message.to.len() > MAX_RECIPIENTS {
        return Err(MailError::INVALID);
    }
    if message.subject.chars().any(char::is_control) {
        return Err(MailError::INVALID);
    }
    let message_id = match &message.message_id {
        Some(id)
            if !id.is_empty()
                && id.len() <= 200
                && id.bytes().all(|b| b.is_ascii_graphic() && b != b'<' && b != b'>') =>
        {
            id.clone()
        }
        Some(_) => return Err(MailError::INVALID),
        None => format!("{}@{}", uuid::Uuid::now_v7(), app_host(state)),
    };
    let mut builder = lettre::Message::builder()
        .from(server.from.clone())
        .subject(message.subject.clone())
        .message_id(Some(format!("<{message_id}>")));
    if let Some(reply_to) = &server.reply_to {
        builder = builder.reply_to(reply_to.clone());
    }
    let mut to = Vec::new();
    for recipient in &message.to {
        if recipient.chars().any(char::is_control) {
            return Err(MailError::INVALID);
        }
        let mailbox: Mailbox = recipient.trim().parse().map_err(|_| MailError::INVALID)?;
        to.push(mailbox.email.to_string());
        builder = builder.to(mailbox);
    }
    let email = match &message.html {
        None => builder.singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(message.text.clone())),
        Some(html) => builder.multipart(MultiPart::alternative_plain_html(message.text.clone(), html.clone())),
    }
    .map_err(|_| MailError::INVALID)?;
    Ok((email, Envelope { to, message_id }))
}

fn smtp(
    state: &AppState,
    config: &MailConfig,
    server: &Server,
    roots: &[Certificate],
) -> Result<AsyncSmtpTransport<Tokio1Executor>, MailError> {
    let tls = || {
        let mut params = TlsParameters::builder(server.host.clone());
        for root in roots {
            params = params.add_root_certificate(root.clone());
        }
        params.build_rustls().map_err(|_| MailError::TLS)
    };
    let tls = match server.encryption {
        Encryption::Tls => Tls::Wrapper(tls()?),
        Encryption::Starttls => Tls::Required(tls()?),
        Encryption::None => Tls::None,
    };
    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(server.host.clone())
        .port(server.port)
        .tls(tls)
        .hello_name(ClientId::Domain(app_host(state)))
        .timeout(Some(Duration::from_secs(config.timeout_secs)));
    if !server.username.is_empty() {
        builder =
            builder.credentials(Credentials::new(server.username.clone(), server.password.clone().unwrap_or_default()));
    }
    Ok(builder.build())
}

/// The code of an SMTP error (never its text).
fn classify(err: &lettre::transport::smtp::Error, encryption: Encryption) -> MailError {
    if err.is_timeout() {
        return MailError::TIMEOUT;
    }
    // A client error with encryption asked for: the server offers no STARTTLS.
    // A handshake refused (a certificate not trusted, or for another name)
    // surfaces as invalid data under a connection error.
    if err.is_tls() || (encryption != Encryption::None && (err.is_client() || handshake_refused(err))) {
        return MailError::TLS;
    }
    if let Some(code) = err.status() {
        let text = code.to_string();
        if matches!(text.as_str(), "530" | "534" | "535" | "538") {
            return MailError::AUTH;
        }
    }
    if err.is_permanent() {
        return MailError::REJECTED;
    }
    if err.is_transient() {
        return MailError::TEMPORARY;
    }
    MailError::CONNECT
}

/// Whether an I/O error of kind `InvalidData` (rustls' refusals) caused `err`.
fn handshake_refused(err: &lettre::transport::smtp::Error) -> bool {
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        if cause.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::InvalidData) {
            return true;
        }
        source = cause.source();
    }
    false
}

/// Sends per minute, for this process (a token bucket refilled continuously).
pub(super) struct Limiter {
    per_minute: f64,
    state: Mutex<(f64, Instant)>,
}

/// Longest wait for a token.
const MAX_WAIT: Duration = Duration::from_secs(60);

impl Limiter {
    pub(super) fn new(per_minute: u32) -> Self {
        let per_minute = f64::from(per_minute);
        Self { per_minute, state: Mutex::new((per_minute, Instant::now())) }
    }

    /// Takes a token, waiting at most a minute (then `smtp_throttled`).
    pub(super) async fn acquire(&self) -> Result<(), MailError> {
        let started = Instant::now();
        loop {
            let wait = {
                let mut guard = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let (tokens, at) = *guard;
                let now = Instant::now();
                let tokens =
                    (tokens + now.duration_since(at).as_secs_f64() * self.per_minute / 60.0).min(self.per_minute);
                if tokens >= 1.0 {
                    *guard = (tokens - 1.0, now);
                    return Ok(());
                }
                *guard = (tokens, now);
                Duration::from_secs_f64((1.0 - tokens) * 60.0 / self.per_minute)
            };
            if started.elapsed() + wait > MAX_WAIT {
                return Err(MailError::THROTTLED);
            }
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_rate_limit_waits_then_refuses() {
        // 60 a minute: the 61st waits about a second.
        let limiter = Limiter::new(60);
        for _ in 0..60 {
            limiter.acquire().await.unwrap();
        }
        let started = Instant::now();
        limiter.acquire().await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(900), "{:?}", started.elapsed());
        // One a minute: the second would wait a whole minute, so it is refused at once.
        let slow = Limiter::new(1);
        slow.acquire().await.unwrap();
        assert_eq!(slow.acquire().await, Err(MailError::THROTTLED));
    }
}
