use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use super::{Record, Resource, Stored};
use crate::error::Result;

pub enum Condition {
    Equals(&'static str, Value),
    Contains(&'static str, String),
    GreaterThan(&'static str, Value),
    LessThan(&'static str, Value),
    Includes(&'static str, Value),
    Missing(&'static str),
}

#[async_trait]
pub trait Repository<T: Record>: Send + Sync {
    async fn search(&self, conditions: &[Condition]) -> Result<Vec<Stored<T>>>;
    async fn find(&self, id: Uuid) -> Result<Stored<T>>;
    async fn insert(&self, stored: &Stored<T>) -> Result<()>;
    async fn update(&self, stored: &Stored<T>) -> Result<()>;
    async fn delete(&self, id: Uuid) -> Result<()>;
    async fn add_to_list(&self, id: Uuid, list: &'static str, item: Value) -> Result<()>;
    async fn remove_from_list(&self, id: Uuid, list: &'static str, item: Value) -> Result<()>;
}

pub struct Service<T: Resource>(Arc<dyn Repository<T>>);

impl<T: Resource> Clone for Service<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: Resource> Service<T> {
    pub fn new(repository: impl Repository<T> + 'static) -> Self {
        Self(Arc::new(repository))
    }

    #[tracing::instrument(name = "service.search", skip_all, fields(collection = T::COLLECTION))]
    pub async fn search(&self, conditions: &[Condition]) -> Result<Vec<Stored<T>>> {
        self.0.search(conditions).await
    }

    #[tracing::instrument(name = "service.find", skip_all, fields(collection = T::COLLECTION))]
    pub async fn find(&self, id: Uuid) -> Result<Stored<T>> {
        self.0.find(id).await
    }

    #[tracing::instrument(name = "service.create", skip_all, fields(collection = T::COLLECTION))]
    pub async fn create(&self, value: T) -> Result<Stored<T>> {
        let stored = Stored::valid(Uuid::now_v7(), value)?;
        self.0.insert(&stored).await?;
        Ok(stored)
    }

    #[tracing::instrument(name = "service.replace", skip_all, fields(collection = T::COLLECTION))]
    pub async fn replace(&self, id: Uuid, value: T) -> Result<Stored<T>> {
        let stored = Stored::valid(id, value)?;
        self.0.update(&stored).await?;
        Ok(stored)
    }

    #[tracing::instrument(name = "service.delete", skip_all, fields(collection = T::COLLECTION))]
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        self.0.delete(id).await
    }

    #[tracing::instrument(name = "service.add_to_list", skip_all, fields(collection = T::COLLECTION))]
    pub async fn add_to_list(&self, id: Uuid, list: &'static str, item: Value) -> Result<()> {
        self.0.add_to_list(id, list, item).await
    }

    #[tracing::instrument(name = "service.remove_from_list", skip_all, fields(collection = T::COLLECTION))]
    pub async fn remove_from_list(&self, id: Uuid, list: &'static str, item: Value) -> Result<()> {
        self.0.remove_from_list(id, list, item).await
    }
}
