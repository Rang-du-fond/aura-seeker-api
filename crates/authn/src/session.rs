use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{Account, Settings};

const REFRESH_TOKEN_PREFIX: &str = "rt_";
const REFRESH_TOKEN_BYTES: usize = 32;

#[derive(Default)]
pub struct Device {
    pub label: Option<String>,
    pub user_agent: Option<String>,
    pub ip: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub user_id: Uuid,
    pub methods: String,
    pub auth_time: u64,
    pub security_version: u32,
    pub device_label: Option<String>,
    pub user_agent: Option<String>,
    pub ip_created: Option<String>,
    pub last_used_at: u64,
    pub idle_expires_at: u64,
    pub absolute_expires_at: u64,
    pub revoked_at: Option<u64>,
    pub revoke_reason: Option<String>,
}

impl Session {
    #[must_use]
    pub fn started(account: &Account, method: &str, device: Device, now: u64, settings: &Settings) -> Self {
        Self {
            user_id: account.id,
            methods: method.into(),
            auth_time: now,
            security_version: account.security_version,
            device_label: device.label,
            user_agent: device.user_agent,
            ip_created: device.ip,
            last_used_at: now,
            idle_expires_at: now + settings.refresh_idle_ttl.as_secs(),
            absolute_expires_at: now + settings.refresh_absolute_ttl.as_secs(),
            revoked_at: None,
            revoke_reason: None,
        }
    }

    #[must_use]
    pub const fn is_usable(&self, now: u64) -> bool {
        self.revoked_at.is_none() && now < self.idle_expires_at && now < self.absolute_expires_at
    }

    #[must_use]
    pub fn used(self, now: u64, settings: &Settings) -> Self {
        let idle_expires_at = self.absolute_expires_at.min(now + settings.refresh_idle_ttl.as_secs());
        Self { last_used_at: now, idle_expires_at, ..self }
    }

    #[must_use]
    pub fn revoked(self, reason: &str, now: u64) -> Self {
        Self { revoked_at: Some(now), revoke_reason: Some(reason.into()), ..self }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ActiveSession {
    id: Uuid,
    device_label: Option<String>,
    user_agent: Option<String>,
    ip: Option<String>,
    signed_in_at: u64,
    last_used_at: u64,
    current: bool,
}

impl ActiveSession {
    #[must_use]
    pub fn describing(id: Uuid, session: Session, current: Uuid) -> Self {
        Self {
            id,
            device_label: session.device_label,
            user_agent: session.user_agent,
            ip: session.ip_created,
            signed_in_at: session.auth_time,
            last_used_at: session.last_used_at,
            current: id == current,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct RefreshToken {
    pub session_id: Uuid,
    pub token_hash: String,
    pub created_at: u64,
    pub replaced_at: Option<u64>,
    pub replaced_by: Option<Uuid>,
}

impl RefreshToken {
    #[must_use]
    pub fn generated(session_id: Uuid, now: u64) -> (String, Self) {
        let mut secret = [0_u8; REFRESH_TOKEN_BYTES];
        OsRng.fill_bytes(&mut secret);
        let token = format!("{REFRESH_TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(secret));
        let record =
            Self { session_id, token_hash: hash(&token), created_at: now, replaced_at: None, replaced_by: None };
        (token, record)
    }

    #[must_use]
    pub fn is_reused(&self, now: u64, settings: &Settings) -> bool {
        self.replaced_at.is_some_and(|replaced_at| now > replaced_at + settings.refresh_retry_grace.as_secs())
    }

    #[must_use]
    pub fn replaced(self, successor: Uuid, now: u64) -> Self {
        Self { replaced_at: Some(now), replaced_by: Some(successor), ..self }
    }
}

#[must_use]
pub fn hash(refresh_token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(refresh_token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_refresh_token_is_reused_only_after_the_retry_grace() {
        let settings = Settings::default();
        let (token, record) = RefreshToken::generated(Uuid::nil(), 1_000);
        assert!(token.starts_with(REFRESH_TOKEN_PREFIX));
        assert_eq!(record.token_hash, hash(&token));
        assert!(!record.is_reused(5_000, &settings));
        let replaced = record.replaced(Uuid::nil(), 2_000);
        assert!(!replaced.is_reused(2_000 + settings.refresh_retry_grace.as_secs(), &settings));
        assert!(replaced.is_reused(2_001 + settings.refresh_retry_grace.as_secs(), &settings));
    }
}
