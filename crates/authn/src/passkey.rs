use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    AeadCore, KeyInit, XChaCha20Poly1305,
    aead::{Aead, OsRng},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;
use webauthn_rs::{
    Webauthn, WebauthnBuilder,
    prelude::{
        DiscoverableAuthentication, DiscoverableKey, Passkey, PasskeyRegistration, PublicKeyCredential,
        RegisterPublicKeyCredential, Url,
    },
};
use webauthn_rs_proto::ResidentKeyRequirement;

use crate::{Account, Error, Result, Settings};

const CEREMONY_LIFETIME: u64 = 300;
const NONCE_LENGTH: usize = 24;

#[derive(Serialize, Deserialize)]
pub struct PasskeyRecord {
    pub user_id: Uuid,
    pub credential_id: String,
    pub passkey: String,
    pub label: Option<String>,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
}

#[derive(Serialize, ToSchema)]
pub struct PasskeySummary {
    id: Uuid,
    label: Option<String>,
    created_at: u64,
    last_used_at: Option<u64>,
}

impl PasskeySummary {
    #[must_use]
    pub fn describing(id: Uuid, record: &PasskeyRecord) -> Self {
        Self { id, label: record.label.clone(), created_at: record.created_at, last_used_at: record.last_used_at }
    }
}

#[derive(Serialize, ToSchema)]
pub struct Ceremony {
    #[schema(value_type = Object)]
    options: Value,
    blob: String,
}

#[derive(Serialize, Deserialize)]
struct Sealed<S> {
    expires_at: u64,
    user: Option<Uuid>,
    state: S,
}

pub struct Passkeys {
    webauthn: Webauthn,
    cipher: XChaCha20Poly1305,
}

impl Passkeys {
    pub fn configured(settings: &Settings) -> Result<Self> {
        let webauthn = &settings.webauthn;
        let origins = webauthn.origins.iter().map(|origin| Url::parse(origin).map_err(Error::unexpected));
        let origins = origins.collect::<Result<Vec<_>>>()?;
        let on_domain = origins.iter().find(|origin| is_on_domain(origin, &webauthn.rp_id)).ok_or_else(|| {
            let problem = format!(
                "auth.webauthn: none of the origins {:?} is on the rp_id domain {:?}; \
                 rp_id must be a bare domain (no scheme, no port) and at least one origin must be on it or on a subdomain",
                webauthn.origins, webauthn.rp_id
            );
            Error::unexpected(problem)
        })?;
        let builder =
            WebauthnBuilder::new(&webauthn.rp_id, on_domain).map_err(Error::unexpected)?.rp_name(&webauthn.rp_name);
        let builder = origins.iter().fold(builder, WebauthnBuilder::append_allowed_origin);
        let key = settings.keys.secret(&settings.keys.aead_key_env, "aead")?;
        Ok(Self { webauthn: builder.build().map_err(Error::unexpected)?, cipher: XChaCha20Poly1305::new(&key.into()) })
    }

    pub fn start_registration(&self, account: &Account, existing: &[PasskeyRecord], now: u64) -> Result<Ceremony> {
        let excluded =
            existing.iter().filter_map(|record| Some(parsed(&record.passkey).ok()?.cred_id().clone())).collect();
        let (mut options, state) = self
            .webauthn
            .start_passkey_registration(account.id, &account.email, &account.email, Some(excluded))
            .map_err(Error::unexpected)?;
        if let Some(selection) = &mut options.public_key.authenticator_selection {
            selection.resident_key = Some(ResidentKeyRequirement::Required);
            selection.require_resident_key = true;
        }
        self.ceremony(&options, Some(account.id), &state, now)
    }

    pub fn finish_registration(
        &self,
        user: Uuid,
        blob: &str,
        credential: &RegisterPublicKeyCredential,
        now: u64,
    ) -> Result<(String, String)> {
        let (owner, state): (_, PasskeyRegistration) = self.open(blob, now)?;
        if owner != Some(user) {
            return Err(Error::InvalidPasskey);
        }
        let passkey =
            self.webauthn.finish_passkey_registration(credential, &state).map_err(|_| Error::InvalidPasskey)?;
        Ok((URL_SAFE_NO_PAD.encode(passkey.cred_id()), serde_json::to_string(&passkey).map_err(Error::unexpected)?))
    }

    pub fn start_login(&self, now: u64) -> Result<Ceremony> {
        let (options, state) = self.webauthn.start_discoverable_authentication().map_err(Error::unexpected)?;
        self.ceremony(&options, None, &state, now)
    }

    pub fn claimed_credential(&self, credential: &PublicKeyCredential) -> Result<(Uuid, String)> {
        let (user, credential_id) =
            self.webauthn.identify_discoverable_authentication(credential).map_err(|_| Error::InvalidPasskey)?;
        Ok((user, URL_SAFE_NO_PAD.encode(credential_id)))
    }

    pub fn finish_login(&self, blob: &str, credential: &PublicKeyCredential, stored: &str, now: u64) -> Result<String> {
        let (_, state): (_, DiscoverableAuthentication) = self.open(blob, now)?;
        let mut passkey = parsed(stored)?;
        let outcome = self
            .webauthn
            .finish_discoverable_authentication(credential, state, &[DiscoverableKey::from(&passkey)])
            .map_err(|_| Error::InvalidPasskey)?;
        passkey.update_credential(&outcome);
        serde_json::to_string(&passkey).map_err(Error::unexpected)
    }

    fn ceremony(
        &self,
        options: &impl Serialize,
        user: Option<Uuid>,
        state: &impl Serialize,
        now: u64,
    ) -> Result<Ceremony> {
        let sealed = Sealed { expires_at: now + CEREMONY_LIFETIME, user, state };
        let plaintext = serde_json::to_vec(&sealed).map_err(Error::unexpected)?;
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext =
            self.cipher.encrypt(&nonce, plaintext.as_ref()).map_err(|cause| Error::unexpected(cause.to_string()))?;
        Ok(Ceremony {
            options: serde_json::to_value(options).map_err(Error::unexpected)?,
            blob: URL_SAFE_NO_PAD.encode([&nonce[..], &ciphertext].concat()),
        })
    }

    fn open<S: DeserializeOwned>(&self, blob: &str, now: u64) -> Result<(Option<Uuid>, S)> {
        let bytes = URL_SAFE_NO_PAD.decode(blob).map_err(|_| Error::InvalidPasskey)?;
        let (nonce, ciphertext) = bytes.split_at_checked(NONCE_LENGTH).ok_or(Error::InvalidPasskey)?;
        let plaintext = self.cipher.decrypt(nonce.into(), ciphertext).map_err(|_| Error::InvalidPasskey)?;
        let sealed: Sealed<S> = serde_json::from_slice(&plaintext).map_err(|_| Error::InvalidPasskey)?;
        if now < sealed.expires_at { Ok((sealed.user, sealed.state)) } else { Err(Error::ChallengeExpired) }
    }
}

fn is_on_domain(origin: &Url, domain: &str) -> bool {
    origin.domain().is_some_and(|host| host == domain || host.ends_with(&format!(".{domain}")))
}

fn parsed(passkey: &str) -> Result<Passkey> {
    serde_json::from_str(passkey).map_err(Error::unexpected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_domain(origin: &str, domain: &str) -> Result<bool> {
        Ok(is_on_domain(&Url::parse(origin).map_err(Error::unexpected)?, domain))
    }

    #[test]
    fn an_origin_is_on_a_domain_when_it_is_that_domain_or_a_subdomain() -> Result<()> {
        assert!(on_domain("https://auraseeker.fr", "auraseeker.fr")?);
        assert!(on_domain("https://api.auraseeker.fr", "auraseeker.fr")?);
        assert!(on_domain("http://localhost:8080", "localhost")?);
        assert!(!on_domain("https://auraseeker.fr", "api.auraseeker.fr")?);
        assert!(!on_domain("https://notauraseeker.fr", "auraseeker.fr")?);
        assert!(!on_domain("https://aura-seeker.matheo-galuba.com", "auraseeker.fr")?);
        assert!(!on_domain("https://auraseeker.fr", "https://auraseeker.fr")?);
        assert!(!on_domain("android:apk-key-hash:abc", "auraseeker.fr")?);
        Ok(())
    }
}
