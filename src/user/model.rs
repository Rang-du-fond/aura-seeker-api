use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::crud::Record;

pub const PASSWORD_PROVIDER: &str = "password";

#[derive(Serialize, Deserialize, Validate)]
pub struct User {
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub role: String,
    #[serde(default)]
    pub security_version: u32,
    #[serde(default)]
    pub email_verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Record for User {
    const COLLECTION: &'static str = "users";
}

#[derive(Serialize, Deserialize, Validate)]
pub struct Identity {
    pub user_id: Uuid,
    pub provider: String,
    pub subject: String,
    #[serde(default)]
    pub provider_email: Option<String>,
    #[serde(default, deserialize_with = "flag")]
    pub provider_email_verified: bool,
    pub password_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

fn flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    let stored = Value::deserialize(deserializer)?;
    Ok(stored.as_bool().unwrap_or_else(|| stored.as_i64() == Some(1)))
}

impl Record for Identity {
    const COLLECTION: &'static str = "identities";
}

#[derive(Serialize, Deserialize, Validate, ToSchema)]
pub struct Profile {
    #[validate(length(min = 1, max = 64))]
    pub display_name: Option<String>,
}

impl Record for Profile {
    const COLLECTION: &'static str = User::COLLECTION;
}
