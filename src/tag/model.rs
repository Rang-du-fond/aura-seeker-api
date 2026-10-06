use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

use crate::crud::Record;

#[derive(Serialize, Deserialize, Validate, ToSchema)]
pub struct Tag {
    #[validate(length(min = 1))]
    pub name: String,
}

impl Record for Tag {
    const COLLECTION: &'static str = "tags";
}

#[derive(Serialize, Deserialize, Validate, ToSchema)]
pub struct TagUsage {
    #[serde(flatten)]
    pub tag: Tag,
    pub places: usize,
}

impl Record for TagUsage {
    const COLLECTION: &'static str = Tag::COLLECTION;
}
