use async_trait::async_trait;
use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, path::Path, prefix::PrefixStore};
use url::Url;
use uuid::Uuid;

use super::BlobStore;
use crate::error::{Error, Result};

pub struct ObjectStorage(Box<dyn ObjectStore>);

impl ObjectStorage {
    pub fn open(url: &str) -> Result<Self> {
        let url = Url::parse(url).map_err(Error::unexpected)?;
        let (store, prefix) = object_store::parse_url(&url).map_err(failure)?;
        Ok(Self(Box::new(PrefixStore::new(store, prefix))))
    }
}

#[async_trait]
impl BlobStore for ObjectStorage {
    #[tracing::instrument(name = "storage.write", skip_all, fields(%key))]
    async fn write(&self, key: Uuid, content: Bytes) -> Result<()> {
        self.0.put(&location(key), content.into()).await.map(drop).map_err(failure)
    }

    #[tracing::instrument(name = "storage.read", skip_all, fields(%key))]
    async fn read(&self, key: Uuid) -> Result<Bytes> {
        self.0.get(&location(key)).await.map_err(failure)?.bytes().await.map_err(failure)
    }

    #[tracing::instrument(name = "storage.delete", skip_all, fields(%key))]
    async fn delete(&self, key: Uuid) -> Result<()> {
        self.0.delete(&location(key)).await.map_err(failure)
    }
}

fn location(key: Uuid) -> Path {
    Path::from(key.to_string())
}

fn failure(error: object_store::Error) -> Error {
    if matches!(error, object_store::Error::NotFound { .. }) { Error::NotFound } else { Error::unexpected(error) }
}
