use async_trait::async_trait;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{ExternalIdentity, Result};

pub struct Account {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub role: String,
    pub security_version: u32,
    pub email_verified: bool,
    pub password_hash: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct LinkedIdentity {
    pub id: Uuid,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip)]
    pub subject: String,
}

#[async_trait]
pub trait Accounts: Send + Sync {
    async fn find(&self, id: Uuid) -> Result<Option<Account>>;
    async fn find_by_email(&self, email: &str) -> Result<Option<Account>>;
    async fn find_by_identity(&self, provider: &str, subject: &str) -> Result<Option<Account>>;
    async fn identities(&self, id: Uuid) -> Result<Vec<LinkedIdentity>>;
    async fn link_identity(&self, id: Uuid, identity: &ExternalIdentity) -> Result<()>;
    async fn unlink_identity(&self, identity: Uuid) -> Result<()>;
    async fn remove_password(&self, id: Uuid) -> Result<()>;
    async fn register(&self, account: &Account) -> Result<()>;
    async fn increment_security_version(&self, id: Uuid) -> Result<()>;
    async fn mark_email_verified(&self, id: Uuid) -> Result<()>;
    async fn set_password(&self, id: Uuid, password_hash: &str) -> Result<()>;
}
