use serde::{Deserialize, Serialize, de::DeserializeOwned};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::error::Result;

pub trait Record: Serialize + DeserializeOwned + Send + Sync + 'static {
    const COLLECTION: &'static str;
    const LISTS: &'static [&'static str] = &[];
}

pub trait Resource: Record + Validate {}

impl<T: Record + Validate> Resource for T {}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct Stored<T> {
    pub id: Uuid,
    #[serde(flatten)]
    pub value: T,
}

impl<T: Resource> Stored<T> {
    pub fn valid(id: Uuid, value: T) -> Result<Self> {
        value.validate()?;
        Ok(Self { id, value })
    }
}
