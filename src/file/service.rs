use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use uuid::Uuid;

use super::File;
use crate::{
    crud::{Service, Stored},
    error::{Error, Result},
};

#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn write(&self, key: Uuid, content: Bytes) -> Result<()>;
    async fn read(&self, key: Uuid) -> Result<Bytes>;
    async fn delete(&self, key: Uuid) -> Result<()>;
}

#[derive(Clone)]
pub struct FileService {
    pub metadata: Service<File>,
    blobs: Arc<dyn BlobStore>,
}

impl FileService {
    pub fn new(metadata: Service<File>, blobs: impl BlobStore + 'static) -> Self {
        Self { metadata, blobs: Arc::new(blobs) }
    }

    #[tracing::instrument(name = "file.upload", skip_all)]
    pub async fn upload(&self, media_type: String, content: Bytes) -> Result<Stored<File>> {
        let size = u64::try_from(content.len()).map_err(Error::unexpected)?;
        let file = self.metadata.create(File { media_type, size }).await?;
        self.blobs.write(file.id, content).await?;
        Ok(file)
    }

    #[tracing::instrument(name = "file.download", skip_all)]
    pub async fn download(&self, id: Uuid) -> Result<(File, Bytes)> {
        Ok((self.metadata.find(id).await?.value, self.blobs.read(id).await?))
    }

    #[tracing::instrument(name = "file.delete", skip_all)]
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        self.metadata.delete(id).await?;
        self.blobs.delete(id).await
    }
}
