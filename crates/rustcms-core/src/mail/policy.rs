//! The operator's policy for SMTP servers (`[mail]` `allowed_hosts`,
//! `allowed_ports`, `allow_insecure`): checked when the configuration is
//! loaded, when the panel saves the mail settings, and at every send.

use serde::{Deserialize, Serialize};

/// How the connection is secured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Encryption {
    /// Plain connection upgraded with STARTTLS, which the server must offer.
    #[default]
    Starttls,
    /// TLS from the first byte (port 465).
    Tls,
    /// None (`allow_insecure` only, without authentication).
    None,
}

/// See the module.
#[derive(Debug, Clone, Default)]
pub struct MailPolicy {
    /// Names, `*.domain` or `*`; empty: any (outside production).
    pub allowed_hosts: Vec<String>,
    /// Ports.
    pub allowed_ports: Vec<u16>,
    /// Whether `Encryption::None` is allowed.
    pub allow_insecure: bool,
}

/// What is wrong, by field (`host`, `port`, `encryption`, `username`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyIssue {
    /// Field.
    pub field: &'static str,
    /// Message (an issue template).
    pub message: &'static str,
}

fn issue(field: &'static str, message: &'static str) -> Result<(), PolicyIssue> {
    Err(PolicyIssue { field, message })
}

/// Whether `host` is a DNS name (letters, digits, `-`, dots) or an IP address.
pub(crate) fn valid_host(host: &str) -> bool {
    if host.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    (1..=253).contains(&host.len())
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

impl MailPolicy {
    /// Whether `host` (lower case) is allowed.
    pub fn allows_host(&self, host: &str) -> bool {
        self.allowed_hosts.is_empty()
            || self.allowed_hosts.iter().any(|allowed| {
                let allowed = allowed.to_ascii_lowercase();
                allowed == "*"
                    || allowed == host
                    || allowed.strip_prefix("*.").is_some_and(|domain| host.ends_with(&format!(".{domain}")))
            })
    }

    /// Checks a server: a valid host the policy allows, an allowed port,
    /// encryption unless insecure is allowed, and no authentication without it.
    pub fn check(&self, host: &str, port: u16, encryption: Encryption, authenticates: bool) -> Result<(), PolicyIssue> {
        let host = host.trim().to_ascii_lowercase();
        if host.is_empty() {
            return issue("host", "is required");
        }
        if !valid_host(&host) {
            return issue("host", "is invalid");
        }
        if !self.allows_host(&host) {
            return issue("host", "is not allowed");
        }
        if port == 0 || !self.allowed_ports.contains(&port) {
            return issue("port", "is not allowed");
        }
        if encryption == Encryption::None {
            if !self.allow_insecure {
                return issue("encryption", "is not allowed");
            }
            if authenticates {
                return issue("username", "is not allowed");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_ports_and_tls_follow_the_policy() {
        let policy = MailPolicy {
            allowed_hosts: vec!["smtp.example.com".into(), "*.mail.test".into()],
            allowed_ports: vec![465, 587],
            allow_insecure: false,
        };
        assert!(policy.check("SMTP.example.com", 587, Encryption::Starttls, true).is_ok());
        assert!(policy.check("a.mail.test", 465, Encryption::Tls, true).is_ok());
        let field = |r: Result<(), PolicyIssue>| r.unwrap_err().field;
        assert_eq!(field(policy.check("mail.test", 587, Encryption::Starttls, true)), "host");
        assert_eq!(field(policy.check("evil.com", 587, Encryption::Starttls, true)), "host");
        assert_eq!(field(policy.check("bad host", 587, Encryption::Starttls, true)), "host");
        assert_eq!(field(policy.check("smtp.example.com", 25, Encryption::Starttls, true)), "port");
        assert_eq!(field(policy.check("smtp.example.com", 587, Encryption::None, false)), "encryption");
        let insecure = MailPolicy { allow_insecure: true, ..policy };
        assert!(insecure.check("smtp.example.com", 587, Encryption::None, false).is_ok());
        assert_eq!(field(insecure.check("smtp.example.com", 587, Encryption::None, true)), "username");
        let any = MailPolicy { allowed_hosts: vec!["*".into()], allowed_ports: vec![587], allow_insecure: false };
        assert!(any.check("10.0.0.1", 587, Encryption::Starttls, true).is_ok());
    }
}
