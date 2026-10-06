use std::{collections::HashMap, time::Duration};

use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header, get_current_timestamp as now, jwk::JwkSet,
};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::{Error, Result, Settings};

pub const GOOGLE: &str = "google";
const GOOGLE_ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];
const KEY_REFRESH_INTERVAL: u64 = 60;
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct ExternalIdentity {
    pub provider: &'static str,
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
struct IdTokenClaims {
    sub: String,
    email: Option<String>,
    #[serde(default)]
    email_verified: bool,
    name: Option<String>,
    nonce: Option<String>,
}

#[derive(Default)]
struct SigningKeys {
    by_key_id: HashMap<String, DecodingKey>,
    fetched_at: u64,
}

pub struct OidcProvider {
    id: &'static str,
    issuers: Vec<String>,
    audiences: Vec<String>,
    keys_url: String,
    client: reqwest::Client,
    keys: Mutex<SigningKeys>,
}

impl OidcProvider {
    pub fn google(settings: &Settings) -> Result<Self> {
        let google = &settings.providers.google;
        let web = Some(&google.client_id_web).filter(|client_id| !client_id.is_empty());
        Ok(Self {
            id: GOOGLE,
            issuers: GOOGLE_ISSUERS.map(Into::into).into(),
            audiences: web.into_iter().chain(&google.client_ids_native).cloned().collect(),
            keys_url: google.keys_url.clone(),
            client: reqwest::Client::builder().timeout(TIMEOUT).build().map_err(Error::unexpected)?,
            keys: Mutex::default(),
        })
    }

    #[tracing::instrument(name = "auth.verify_id_token", skip_all, fields(provider = self.id))]
    pub async fn verify_id_token(&self, id_token: &str, nonce: &str) -> Result<ExternalIdentity> {
        let key_id = decode_header(id_token).ok().and_then(|header| header.kid).ok_or(Error::InvalidIdToken)?;
        let key = self.signing_key(&key_id).await?.ok_or(Error::InvalidIdToken)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&self.issuers);
        validation.set_audience(&self.audiences);
        let claims = decode::<IdTokenClaims>(id_token, &key, &validation).map_err(|_| Error::InvalidIdToken)?.claims;
        if claims.nonce.as_deref() != Some(nonce) {
            return Err(Error::InvalidIdToken);
        }
        Ok(ExternalIdentity {
            provider: self.id,
            subject: claims.sub,
            email: claims.email,
            email_verified: claims.email_verified,
            display_name: claims.name,
        })
    }

    async fn signing_key(&self, key_id: &str) -> Result<Option<DecodingKey>> {
        let mut keys = self.keys.lock().await;
        if !keys.by_key_id.contains_key(key_id) && now() >= keys.fetched_at + KEY_REFRESH_INTERVAL {
            *keys = self.published_keys().await?;
        }
        let key = keys.by_key_id.get(key_id).cloned();
        drop(keys);
        Ok(key)
    }

    async fn published_keys(&self) -> Result<SigningKeys> {
        let response = self.client.get(&self.keys_url).send().await.map_err(Error::unexpected)?;
        let document =
            response.error_for_status().map_err(Error::unexpected)?.text().await.map_err(Error::unexpected)?;
        let published: JwkSet = serde_json::from_str(&document).map_err(Error::unexpected)?;
        let usable = published
            .keys
            .iter()
            .filter_map(|key| Some((key.common.key_id.clone()?, DecodingKey::from_jwk(key).ok()?)));
        Ok(SigningKeys { by_key_id: usable.collect(), fetched_at: now() })
    }
}
