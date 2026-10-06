use std::time::Duration;

use sha1::{Digest, Sha1};

use crate::{Error, Result, Settings};

const PREFIX_LENGTH: usize = 5;
const TIMEOUT: Duration = Duration::from_secs(3);

pub struct Breaches {
    client: reqwest::Client,
    range_url: Option<String>,
}

impl Breaches {
    pub fn configured(settings: &Settings) -> Result<Self> {
        let client = reqwest::Client::builder().timeout(TIMEOUT).build().map_err(Error::unexpected)?;
        let range_url = settings.check_breached_passwords.then(|| settings.breached_passwords_url.clone());
        Ok(Self { client, range_url })
    }

    #[tracing::instrument(name = "auth.breached_password_check", skip_all)]
    pub async fn check(&self, password: &str) -> Result<()> {
        let Some(range_url) = &self.range_url else {
            return Ok(());
        };
        let digest = Sha1::digest(password).iter().fold(String::new(), |hex, byte| format!("{hex}{byte:02X}"));
        let (prefix, suffix) = digest.split_at(PREFIX_LENGTH);
        let found = self.breached_suffixes(&format!("{range_url}{prefix}")).await;
        let breached = found.map_or_else(
            |cause| accept_unchecked(&cause),
            |suffixes| suffixes.lines().any(|line| line.split(':').next() == Some(suffix)),
        );
        if breached { Err(Error::BreachedPassword) } else { Ok(()) }
    }

    async fn breached_suffixes(&self, url: &str) -> reqwest::Result<String> {
        self.client.get(url).send().await?.error_for_status()?.text().await
    }
}

fn accept_unchecked(cause: &reqwest::Error) -> bool {
    tracing::warn!(%cause, "breached password check unavailable, the password is accepted unchecked");
    false
}
