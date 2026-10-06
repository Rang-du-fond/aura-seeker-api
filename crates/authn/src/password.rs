use std::sync::LazyLock;

use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use tokio::task::spawn_blocking;

use crate::{Error, Result};

const LENGTH: std::ops::RangeInclusive<usize> = 12..=128;

static UNKNOWN_ACCOUNT_HASH: LazyLock<String> = LazyLock::new(|| hash_blocking("unknown account").unwrap_or_default());

pub fn check_policy(password: &str) -> Result<()> {
    LENGTH.contains(&password.chars().count()).then_some(()).ok_or(Error::InvalidPassword)
}

pub async fn hash(password: String) -> Result<String> {
    spawn_blocking(move || hash_blocking(&password)).await.map_err(Error::unexpected)?
}

pub async fn matches(password: String, hash: Option<String>) -> Result<bool> {
    spawn_blocking(move || {
        let hash = hash.as_deref().unwrap_or(&UNKNOWN_ACCOUNT_HASH);
        let parsed = PasswordHash::new(hash).map_err(|cause| Error::unexpected(cause.to_string()))?;
        Ok(Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
    })
    .await
    .map_err(Error::unexpected)?
}

pub fn is_outdated(hash: &str) -> bool {
    let current = Params::default();
    let parameters = PasswordHash::new(hash).ok().and_then(|parsed| {
        let algorithm = Algorithm::try_from(parsed.algorithm).ok()?;
        Some((algorithm, Params::try_from(&parsed).ok()?))
    });
    parameters.is_none_or(|(algorithm, used)| {
        algorithm != Algorithm::default()
            || (used.m_cost(), used.t_cost(), used.p_cost()) != (current.m_cost(), current.t_cost(), current.p_cost())
    })
}

fn hash_blocking(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|hash| hash.to_string())
        .map_err(|cause| Error::unexpected(cause.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEAKER_HASH: &str =
        "$argon2id$v=19$m=4096,t=1,p=1$c2FsdHNhbHRzYWx0$Gm5EKLPHUZkrpM1Tsn7LZ1ZDzVMyy2yWLAxsCPe/1mE";

    #[test]
    fn the_policy_only_constrains_the_length() {
        assert!(check_policy("short").is_err());
        assert!(check_policy("twelve chars").is_ok());
        assert!(check_policy(&"x".repeat(129)).is_err());
    }

    #[test]
    fn a_hash_verifies_its_password_and_only_older_parameters_are_outdated() -> Result<()> {
        let hash = hash_blocking("zephyr quilt marmot 42")?;
        let parsed = PasswordHash::new(&hash).map_err(|cause| Error::unexpected(cause.to_string()))?;
        assert!(Argon2::default().verify_password(b"zephyr quilt marmot 42", &parsed).is_ok());
        assert!(Argon2::default().verify_password(b"another password", &parsed).is_err());
        assert!(!is_outdated(&hash));
        assert!(is_outdated(WEAKER_HASH));
        assert!(is_outdated("not a hash"));
        Ok(())
    }
}
