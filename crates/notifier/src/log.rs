use async_trait::async_trait;

use crate::{Notification, Notifier, Result};

pub struct LogNotifier;

impl LogNotifier {
    #[must_use]
    pub fn announced(missing_variable: &str) -> Self {
        tracing::warn!("{missing_variable} is not set: emails will not be sent, only written to this log");
        Self
    }
}

#[async_trait]
impl Notifier for LogNotifier {
    async fn notify(&self, notification: &Notification) -> Result<()> {
        tracing::info!(
            recipient = notification.recipient,
            subject = notification.subject,
            body = notification.body,
            "notification NOT sent, only logged: no SMTP server is configured"
        );
        Ok(())
    }
}
