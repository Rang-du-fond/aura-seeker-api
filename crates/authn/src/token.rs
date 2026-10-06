use ed25519_dalek::{SigningKey, pkcs8::DecodePrivateKey};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header, encode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{Account, Error, Result, Settings, session::Session};

pub const PASSKEY_METHOD: &str = "passkey";
const TOKEN_TYPE: &str = "at+jwt";
const CLOCK_LEEWAY: u64 = 30;
const KEY_ID_BYTES: usize = 8;

#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct Claims {
    pub iss: String,
    pub aud: String,
    pub sub: Uuid,
    pub sid: Uuid,
    pub iat: u64,
    pub exp: u64,
    pub jti: Uuid,
    pub auth_time: u64,
    pub amr: Vec<String>,
    pub mfa: bool,
    pub role: String,
}

pub struct Keyring {
    key_id: String,
    signing: EncodingKey,
    verifying: DecodingKey,
    issuer: String,
    audience: String,
    lifetime: u64,
}

impl Keyring {
    pub fn load(settings: &Settings) -> Result<Self> {
        let file = &settings.keys.signing_keys_file;
        let private_key = std::fs::read_to_string(file).map_err(|cause| {
            Error::unexpected(format!("cannot read the Ed25519 signing key {}: {cause}", file.display()))
        })?;
        let public_key = SigningKey::from_pkcs8_pem(&private_key).map_err(Error::unexpected)?.verifying_key();
        Ok(Self {
            key_id: public_key
                .as_bytes()
                .iter()
                .take(KEY_ID_BYTES)
                .fold(String::new(), |id, byte| format!("{id}{byte:02x}")),
            signing: EncodingKey::from_ed_pem(private_key.as_bytes()).map_err(Error::unexpected)?,
            verifying: DecodingKey::from_ed_der(public_key.as_bytes()),
            issuer: settings.issuer.clone(),
            audience: settings.audience.clone(),
            lifetime: settings.access_token_ttl.as_secs(),
        })
    }

    pub fn sign(&self, account: &Account, session_id: Uuid, session: &Session) -> Result<String> {
        let now = jsonwebtoken::get_current_timestamp();
        let header =
            Header { typ: Some(TOKEN_TYPE.into()), kid: Some(self.key_id.clone()), ..Header::new(Algorithm::EdDSA) };
        let claims = Claims {
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            sub: account.id,
            sid: session_id,
            iat: now,
            exp: now + self.lifetime,
            jti: Uuid::now_v7(),
            auth_time: session.auth_time,
            amr: session.methods.split(',').map(Into::into).collect(),
            mfa: session.methods.split(',').any(|method| method == PASSKEY_METHOD),
            role: account.role.clone(),
        };
        encode(&header, &claims, &self.signing).map_err(Error::unexpected)
    }

    pub fn verify(&self, token: &str) -> Result<Claims> {
        let header = decode_header(token).map_err(|_| Error::InvalidToken)?;
        if header.typ.as_deref() != Some(TOKEN_TYPE) || header.kid.as_ref() != Some(&self.key_id) {
            return Err(Error::InvalidToken);
        }
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.leeway = CLOCK_LEEWAY;
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        decode(token, &self.verifying, &validation).map(|token| token.claims).map_err(|_| Error::InvalidToken)
    }
}
