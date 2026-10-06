use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::crud::Record;

#[derive(Serialize, Deserialize, Validate, ToSchema)]
pub struct File {
    pub media_type: String,
    pub size: u64,
}

impl Record for File {
    const COLLECTION: &'static str = "files";
}
