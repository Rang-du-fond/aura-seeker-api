use std::{path::PathBuf, time::Duration};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{Error, Result};

const MINUTE: u64 = 60;
const DAY: u64 = 24 * 60 * MINUTE;

#[derive(Deserialize)]
#[serde(default)]
pub struct Settings {
    pub issuer: String,
    pub audience: String,
    #[serde(with = "humantime_serde")]
    pub access_token_ttl: Duration,
    #[serde(with = "humantime_serde")]
    pub refresh_idle_ttl: Duration,
    #[serde(with = "humantime_serde")]
    pub refresh_absolute_ttl: Duration,
    #[serde(with = "humantime_serde")]
    pub refresh_retry_grace: Duration,
    #[serde(with = "humantime_serde")]
    pub session_retention: Duration,
    #[serde(with = "humantime_serde")]
    pub session_cleanup_interval: Duration,
    #[serde(with = "humantime_serde")]
    pub email_code_ttl: Duration,
    pub email_code_max_attempts: u32,
    pub check_breached_passwords: bool,
    pub breached_passwords_url: String,
    #[serde(with = "humantime_serde")]
    pub recent_auth_window: Duration,
    pub keys: Keys,
    pub webauthn: WebauthnSettings,
    pub rate_limits: RateLimits,
    pub providers: Providers,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Providers {
    pub google: GoogleSettings,
}

#[derive(Deserialize)]
#[serde(default)]
pub struct GoogleSettings {
    pub client_id_web: String,
    pub client_ids_native: Vec<String>,
    pub keys_url: String,
}

impl Default for GoogleSettings {
    fn default() -> Self {
        Self {
            client_id_web: String::new(),
            client_ids_native: Vec::new(),
            keys_url: "https://www.googleapis.com/oauth2/v3/certs".into(),
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
pub struct RateLimits {
    pub code_sends_per_ip: usize,
    pub code_sends_per_email: usize,
    #[serde(with = "humantime_serde")]
    pub code_sends_window: Duration,
    #[serde(with = "humantime_serde")]
    pub code_send_interval: Duration,
    pub code_checks_per_ip: usize,
    #[serde(with = "humantime_serde")]
    pub code_checks_window: Duration,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            code_sends_per_ip: 10,
            code_sends_per_email: 5,
            code_sends_window: Duration::from_secs(60 * MINUTE),
            code_send_interval: Duration::from_secs(MINUTE),
            code_checks_per_ip: 30,
            code_checks_window: Duration::from_secs(15 * MINUTE),
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
pub struct Keys {
    pub signing_keys_file: PathBuf,
    pub code_pepper_env: String,
    pub aead_key_env: String,
}

impl Keys {
    pub fn secret(&self, variable: &str, purpose: &str) -> Result<[u8; 32]> {
        let material = match std::env::var(variable) {
            Ok(secret) => secret.into_bytes(),
            Err(_) => std::fs::read(&self.signing_keys_file).map_err(Error::unexpected)?,
        };
        Ok(Sha256::new().chain_update(purpose).chain_update(material).finalize().into())
    }
}

#[derive(Deserialize)]
#[serde(default)]
pub struct WebauthnSettings {
    pub rp_id: String,
    pub rp_name: String,
    pub origins: Vec<String>,
}

impl Default for WebauthnSettings {
    fn default() -> Self {
        Self { rp_id: "localhost".into(), rp_name: "Aura Seeker".into(), origins: vec!["http://localhost:8080".into()] }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            issuer: "http://localhost:8080".into(),
            audience: "api".into(),
            access_token_ttl: Duration::from_secs(15 * MINUTE),
            refresh_idle_ttl: Duration::from_secs(90 * DAY),
            refresh_absolute_ttl: Duration::from_secs(365 * DAY),
            refresh_retry_grace: Duration::from_secs(30),
            session_retention: Duration::from_secs(30 * DAY),
            session_cleanup_interval: Duration::from_secs(60 * MINUTE),
            email_code_ttl: Duration::from_secs(10 * MINUTE),
            email_code_max_attempts: 5,
            check_breached_passwords: true,
            breached_passwords_url: "https://api.pwnedpasswords.com/range/".into(),
            recent_auth_window: Duration::from_secs(10 * MINUTE),
            keys: Keys::default(),
            webauthn: WebauthnSettings::default(),
            rate_limits: RateLimits::default(),
            providers: Providers::default(),
        }
    }
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            signing_keys_file: "auth_signing_key.pem".into(),
            code_pepper_env: "AUTH_CODE_PEPPER".into(),
            aead_key_env: "AUTH_AEAD_KEY".into(),
        }
    }
}
