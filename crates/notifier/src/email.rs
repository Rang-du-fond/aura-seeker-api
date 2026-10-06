use std::env;

use async_trait::async_trait;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
};
use serde::Deserialize;

use crate::{Error, Notification, Notifier, Result};

const RESERVED_DOMAINS: [&str; 7] =
    ["example.com", "example.net", "example.org", "example", "test", "invalid", "localhost"];

#[derive(Deserialize)]
#[serde(default)]
pub struct EmailSettings {
    pub from: String,
    pub smtp_url_env: String,
}

impl Default for EmailSettings {
    fn default() -> Self {
        Self { from: "Aura Seeker <no-reply@localhost>".into(), smtp_url_env: "SMTP_URL".into() }
    }
}

pub struct EmailNotifier {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl EmailNotifier {
    pub fn configured(settings: &EmailSettings) -> Result<Option<Self>> {
        let Ok(url) = env::var(&settings.smtp_url_env) else {
            return Ok(None);
        };
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(&url).map_err(Error::caused_by)?.build();
        let from: Mailbox = settings.from.parse().map_err(Error::caused_by)?;
        tracing::info!(server = server(&url), %from, "emails are sent through an SMTP server");
        Ok(Some(Self { transport, from }))
    }
}

#[async_trait]
impl Notifier for EmailNotifier {
    async fn notify(&self, notification: &Notification) -> Result<()> {
        let message = Message::builder()
            .from(self.from.clone())
            .to(notification.recipient.parse().map_err(Error::caused_by)?)
            .subject(notification.subject.as_str())
            .header(ContentType::TEXT_PLAIN)
            .body(notification.body.clone())
            .map_err(Error::caused_by)?;
        self.transport.send(message).await.map(drop).map_err(Error::caused_by)
    }
}

pub struct ExceptTestAddresses<N>(pub N);

#[async_trait]
impl<N: Notifier> Notifier for ExceptTestAddresses<N> {
    async fn notify(&self, notification: &Notification) -> Result<()> {
        if is_reserved_for_tests(&notification.recipient) {
            tracing::info!(
                recipient = notification.recipient,
                "email NOT sent, the address is on a domain reserved for tests"
            );
            return Ok(());
        }
        self.0.notify(notification).await
    }
}

fn server(url: &str) -> &str {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let without_credentials = without_scheme.rsplit('@').next().unwrap_or_default();
    without_credentials.split(['/', '?']).next().unwrap_or_default()
}

fn is_reserved_for_tests(address: &str) -> bool {
    let domain = address.rsplit('@').next().unwrap_or_default().to_lowercase();
    let within = |reserved: &str| domain == reserved || domain.ends_with(&format!(".{reserved}"));
    RESERVED_DOMAINS.iter().any(|reserved| within(reserved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domains_are_recognised_whatever_the_case_or_subdomain() {
        assert!(is_reserved_for_tests("camille@seed.example.com"));
        assert!(is_reserved_for_tests("someone@EXAMPLE.org"));
        assert!(is_reserved_for_tests("dev@localhost"));
        assert!(!is_reserved_for_tests("real.person@auraseeker.fr"));
        assert!(!is_reserved_for_tests("someone@notexample.com"));
    }

    #[test]
    fn the_logged_server_never_shows_credentials() {
        assert_eq!(
            server("smtp://user%40relay:secret@smtp-relay.brevo.com:587?tls=required"),
            "smtp-relay.brevo.com:587"
        );
        assert_eq!(server("smtp://127.0.0.1:2525"), "127.0.0.1:2525");
    }
}
