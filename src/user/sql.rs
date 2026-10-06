use async_trait::async_trait;
use authn::{
    Account, Accounts, Challenge, ExternalIdentity, LinkedIdentity, PasskeyRecord, RefreshToken, Session, Storage,
    Store, Transaction,
};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use super::model::{Identity, PASSWORD_PROVIDER, User};
use crate::{
    crud::{Condition, Record, Repository, SqlConnection, SqlRepository, SqlTransaction, Stored},
    error::Error,
};

impl From<Error> for authn::Error {
    fn from(error: Error) -> Self {
        Self::unexpected(error)
    }
}

impl Record for Session {
    const COLLECTION: &'static str = "sessions";
}

impl Record for RefreshToken {
    const COLLECTION: &'static str = "refresh_tokens";
}

impl Record for Challenge {
    const COLLECTION: &'static str = "email_challenges";
}

impl Record for PasskeyRecord {
    const COLLECTION: &'static str = "passkeys";
}

#[async_trait]
impl Storage for SqlRepository {
    async fn begin(&self) -> authn::Result<Box<dyn Transaction>> {
        Ok(Box::new(self.transaction().await?))
    }
}

#[async_trait]
impl Transaction for SqlTransaction {
    async fn commit(self: Box<Self>) -> authn::Result<()> {
        Ok(self.committed().await?)
    }
}

#[async_trait]
impl<C: SqlConnection> Accounts for SqlRepository<C> {
    async fn find(&self, id: Uuid) -> authn::Result<Option<Account>> {
        self.account(Condition::Equals("id", id.to_string().into())).await
    }

    async fn find_by_email(&self, email: &str) -> authn::Result<Option<Account>> {
        self.account(Condition::Equals("email", email.into())).await
    }

    async fn register(&self, account: &Account) -> authn::Result<()> {
        let now = Utc::now();
        let user = User {
            email: account.email.clone(),
            display_name: account.display_name.clone(),
            role: account.role.clone(),
            security_version: account.security_version,
            email_verified_at: account.email_verified.then_some(now),
            created_at: now,
            updated_at: now,
        };
        let identity = Identity {
            user_id: account.id,
            provider: PASSWORD_PROVIDER.into(),
            subject: account.id.to_string(),
            provider_email: None,
            provider_email_verified: false,
            password_hash: account.password_hash.clone(),
            created_at: now,
        };
        self.insert(&Stored { id: account.id, value: user }).await?;
        if identity.password_hash.is_some() {
            self.insert(&Stored { id: Uuid::now_v7(), value: identity }).await?;
        }
        Ok(())
    }

    async fn set_password(&self, id: Uuid, password_hash: &str) -> authn::Result<()> {
        let password_hash = Some(password_hash.to_owned());
        if let Some(mut identity) = self.password_identity(id).await? {
            identity.value.password_hash = password_hash;
            return Ok(self.update(&identity).await?);
        }
        let identity = Identity {
            user_id: id,
            provider: PASSWORD_PROVIDER.into(),
            subject: id.to_string(),
            provider_email: None,
            provider_email_verified: false,
            password_hash,
            created_at: Utc::now(),
        };
        Ok(self.insert(&Stored { id: Uuid::now_v7(), value: identity }).await?)
    }

    async fn find_by_identity(&self, provider: &str, subject: &str) -> authn::Result<Option<Account>> {
        let identified = [Condition::Equals("provider", provider.into()), Condition::Equals("subject", subject.into())];
        match self.select::<Identity>(&identified).await?.pop() {
            Some(identity) => Accounts::find(self, identity.value.user_id).await,
            None => Ok(None),
        }
    }

    async fn identities(&self, id: Uuid) -> authn::Result<Vec<LinkedIdentity>> {
        let own = self.select::<Identity>(&[Condition::Equals("user_id", id.to_string().into())]).await?;
        let described = own.into_iter().map(|identity| LinkedIdentity {
            id: identity.id,
            provider: identity.value.provider,
            email: identity.value.provider_email,
            subject: identity.value.subject,
        });
        Ok(described.collect())
    }

    async fn unlink_identity(&self, identity: Uuid) -> authn::Result<()> {
        Ok(Repository::<Identity>::delete(self, identity).await?)
    }

    async fn link_identity(&self, id: Uuid, identity: &ExternalIdentity) -> authn::Result<()> {
        let identity = Identity {
            user_id: id,
            provider: identity.provider.into(),
            subject: identity.subject.clone(),
            provider_email: identity.email.clone(),
            provider_email_verified: identity.email_verified,
            password_hash: None,
            created_at: Utc::now(),
        };
        Ok(self.insert(&Stored { id: Uuid::now_v7(), value: identity }).await?)
    }

    async fn remove_password(&self, id: Uuid) -> authn::Result<()> {
        match self.password_identity(id).await? {
            Some(identity) => Ok(Repository::<Identity>::delete(self, identity.id).await?),
            None => Ok(()),
        }
    }

    async fn mark_email_verified(&self, id: Uuid) -> authn::Result<()> {
        let mut user = Repository::<User>::find(self, id).await?;
        user.value.email_verified_at.get_or_insert_with(Utc::now);
        Ok(self.update(&user).await?)
    }

    async fn increment_security_version(&self, id: Uuid) -> authn::Result<()> {
        let mut user = Repository::<User>::find(self, id).await?;
        user.value.security_version += 1;
        user.value.updated_at = Utc::now();
        Ok(self.update(&user).await?)
    }
}

impl<C: SqlConnection> SqlRepository<C> {
    async fn account(&self, user: Condition) -> authn::Result<Option<Account>> {
        let Some(user) = self.select::<User>(&[user]).await?.pop() else {
            return Ok(None);
        };
        let identity = self.password_identity(user.id).await?;
        Ok(Some(Account {
            id: user.id,
            email: user.value.email,
            display_name: user.value.display_name,
            role: user.value.role,
            security_version: user.value.security_version,
            email_verified: user.value.email_verified_at.is_some(),
            password_hash: identity.and_then(|identity| identity.value.password_hash),
        }))
    }

    async fn password_identity(&self, user: Uuid) -> authn::Result<Option<Stored<Identity>>> {
        let password = [
            Condition::Equals("provider", PASSWORD_PROVIDER.into()),
            Condition::Equals("subject", user.to_string().into()),
        ];
        Ok(self.select::<Identity>(&password).await?.pop())
    }
}

#[async_trait]
impl<T: Record, C: SqlConnection> Store<T> for SqlRepository<C> {
    async fn create(&self, id: Uuid, record: T) -> authn::Result<()> {
        Ok(self.insert(&Stored { id, value: record }).await?)
    }

    async fn replace(&self, id: Uuid, record: T) -> authn::Result<()> {
        Ok(self.update(&Stored { id, value: record }).await?)
    }

    async fn replace_if(
        &self,
        id: Uuid,
        record: T,
        field: &'static str,
        expected: Option<Value>,
    ) -> authn::Result<bool> {
        let unchanged = expected.map_or(Condition::Missing(field), |value| Condition::Equals(field, value));
        match self.update_where(&Stored { id, value: record }, &[unchanged]).await {
            Ok(()) => Ok(true),
            Err(Error::NotFound) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    async fn matching(&self, field: &'static str, value: String) -> authn::Result<Vec<(Uuid, T)>> {
        let found = self.select::<T>(&[Condition::Equals(field, value.into())]).await?;
        Ok(found.into_iter().map(|stored| (stored.id, stored.value)).collect())
    }

    async fn remove_before(&self, field: &'static str, threshold: u64) -> authn::Result<u64> {
        Ok(self.delete_where::<T>(&[Condition::LessThan(field, threshold.into())]).await?)
    }

    async fn remove(&self, id: Uuid) -> authn::Result<()> {
        match Repository::<T>::delete(self, id).await {
            Ok(()) | Err(Error::NotFound) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
