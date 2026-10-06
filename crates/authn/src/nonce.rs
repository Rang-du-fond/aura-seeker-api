use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::{Error, Result, Settings};

pub const LIFETIME: u64 = 300;
const RANDOM_BYTES: usize = 16;

pub struct Nonces {
    key: [u8; 32],
}

impl Nonces {
    pub fn configured(settings: &Settings) -> Result<Self> {
        Ok(Self { key: settings.keys.secret(&settings.keys.aead_key_env, "nonce")? })
    }

    pub fn issue(&self, now: u64) -> Result<String> {
        let mut random = [0_u8; RANDOM_BYTES];
        OsRng.fill_bytes(&mut random);
        let payload = format!("{}.{}", now + LIFETIME, URL_SAFE_NO_PAD.encode(random));
        Ok(format!("{payload}.{}", URL_SAFE_NO_PAD.encode(self.mac(&payload)?.finalize().into_bytes())))
    }

    pub fn check(&self, nonce: &str, now: u64) -> Result<()> {
        let (payload, signature) = nonce.rsplit_once('.').ok_or(Error::InvalidIdToken)?;
        let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| Error::InvalidIdToken)?;
        let expires_at = payload.split('.').next().and_then(|expiry| expiry.parse::<u64>().ok());
        let genuine = self.mac(payload)?.verify_slice(&signature).is_ok();
        (genuine && expires_at.is_some_and(|expires_at| now < expires_at)).then_some(()).ok_or(Error::InvalidIdToken)
    }

    fn mac(&self, payload: &str) -> Result<Hmac<Sha256>> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).map_err(Error::unexpected)?;
        mac.update(payload.as_bytes());
        Ok(mac)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nonce_is_accepted_only_untouched_and_before_it_expires() -> Result<()> {
        let nonces = Nonces { key: [7; 32] };
        let nonce = nonces.issue(1_000)?;
        assert!(nonces.check(&nonce, 1_000 + LIFETIME - 1).is_ok());
        assert!(nonces.check(&nonce, 1_000 + LIFETIME).is_err());
        assert!(nonces.check(&nonce.replacen('1', "2", 1), 1_000).is_err());
        assert!(Nonces { key: [8; 32] }.check(&nonce, 1_000).is_err());
        assert!(nonces.check("made-up", 1_000).is_err());
        Ok(())
    }
}
