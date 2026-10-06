use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::{Accounts, Challenge, PasskeyRecord, RefreshToken, Result, Session};

#[async_trait]
pub trait Store<T>: Send + Sync {
    async fn create(&self, id: Uuid, record: T) -> Result<()>;
    async fn replace(&self, id: Uuid, record: T) -> Result<()>;
    async fn replace_if(&self, id: Uuid, record: T, field: &'static str, expected: Option<Value>) -> Result<bool>;
    async fn matching(&self, field: &'static str, value: String) -> Result<Vec<(Uuid, T)>>;
    async fn remove(&self, id: Uuid) -> Result<()>;
    async fn remove_before(&self, field: &'static str, threshold: u64) -> Result<u64>;
}

pub trait Records: Accounts + Store<Session> + Store<RefreshToken> + Store<Challenge> + Store<PasskeyRecord> {}

impl<S: Accounts + Store<Session> + Store<RefreshToken> + Store<Challenge> + Store<PasskeyRecord>> Records for S {}

#[async_trait]
pub trait Storage: Records {
    async fn begin(&self) -> Result<Box<dyn Transaction>>;
}

#[async_trait]
pub trait Transaction: Records {
    async fn commit(self: Box<Self>) -> Result<()>;
}
