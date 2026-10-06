mod email;
mod log;

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

pub use self::{
    email::{EmailNotifier, EmailSettings, ExceptTestAddresses},
    log::LogNotifier,
};

pub type Result<T> = std::result::Result<T, Error>;

type Cause = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, thiserror::Error)]
#[error("the notification could not be sent: {0}")]
pub struct Error(Cause);

impl Error {
    pub fn caused_by(cause: impl Into<Cause>) -> Self {
        Self(cause.into())
    }
}

pub struct Notification {
    pub recipient: String,
    pub subject: String,
    pub body: String,
}

#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify(&self, notification: &Notification) -> Result<()>;
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub email: EmailSettings,
}

pub struct Logged<N>(pub N);

#[async_trait]
impl<N: Notifier> Notifier for Logged<N> {
    #[tracing::instrument(name = "notifier.notify", skip_all)]
    async fn notify(&self, notification: &Notification) -> Result<()> {
        let outcome = self.0.notify(notification).await;
        match &outcome {
            Ok(()) => log_sent(notification),
            Err(cause) => log_failure(notification, cause),
        }
        outcome
    }
}

fn log_sent(notification: &Notification) {
    tracing::info!(recipient = notification.recipient, subject = notification.subject, "notification sent");
}

fn log_failure(notification: &Notification, cause: &Error) {
    tracing::error!(recipient = notification.recipient, subject = notification.subject, %cause, "notification failed");
}

pub fn email_notifier(settings: &Settings) -> Result<Arc<dyn Notifier>> {
    match EmailNotifier::configured(&settings.email)? {
        Some(notifier) => Ok(Arc::new(ExceptTestAddresses(Logged(notifier)))),
        None => Ok(Arc::new(LogNotifier::announced(&settings.email.smtp_url_env))),
    }
}
