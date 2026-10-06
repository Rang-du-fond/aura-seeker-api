use std::env;

use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use crate::{Error, Result, Settings};

pub const LOGIN: &str = "login";
pub const VERIFY_EMAIL: &str = "verify_email";
pub const PASSWORD_RESET: &str = "password_reset";
const CODE_RANGE: u32 = 1_000_000;
const MINIMUM_PEPPER_LENGTH: usize = 32;

#[derive(Clone, Serialize, Deserialize)]
pub struct Challenge {
    pub email: String,
    pub purpose: String,
    pub user_id: Option<Uuid>,
    pub code_hash: String,
    pub attempts: u32,
    pub expires_at: u64,
    pub consumed_at: Option<u64>,
    pub ip: Option<String>,
    pub created_at: u64,
}

impl Challenge {
    #[must_use]
    pub const fn is_open(&self, now: u64) -> bool {
        self.consumed_at.is_none() && now < self.expires_at
    }

    #[must_use]
    pub fn attempted(self) -> Self {
        Self { attempts: self.attempts + 1, ..self }
    }

    #[must_use]
    pub fn consumed(self, now: u64) -> Self {
        Self { consumed_at: Some(now), ..self }
    }
}

pub struct Codes {
    pepper: Vec<u8>,
}

impl Codes {
    pub fn load(settings: &Settings) -> Result<Self> {
        let variable = &settings.keys.code_pepper_env;
        let pepper = env::var(variable).unwrap_or_default().into_bytes();
        if pepper.len() < MINIMUM_PEPPER_LENGTH {
            let problem = format!(
                "{variable} must hold a secret of at least {MINIMUM_PEPPER_LENGTH} characters; \
                 generate one with `openssl rand -base64 32`"
            );
            return Err(Error::unexpected(problem));
        }
        Ok(Self { pepper })
    }

    pub fn generate() -> String {
        format!("{:06}", OsRng.next_u32() % CODE_RANGE)
    }

    pub fn hash(&self, challenge: Uuid, code: &str) -> Result<String> {
        Ok(URL_SAFE_NO_PAD.encode(self.mac(challenge, code)?.finalize().into_bytes()))
    }

    pub fn matches(&self, challenge: Uuid, code: &str, hash: &str) -> Result<bool> {
        let expected = URL_SAFE_NO_PAD.decode(hash).map_err(Error::unexpected)?;
        Ok(self.mac(challenge, code)?.verify_slice(&expected).is_ok())
    }

    fn mac(&self, challenge: Uuid, code: &str) -> Result<Hmac<Sha256>> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.pepper).map_err(Error::unexpected)?;
        mac.update(challenge.as_bytes());
        mac.update(code.trim().as_bytes());
        Ok(mac)
    }
}
