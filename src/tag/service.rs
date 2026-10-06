use serde::Deserialize;
use serde_json::json;
use utoipa::IntoParams;
use uuid::Uuid;

use super::{Tag, model::TagUsage};
use crate::{
    crud::{Condition, Service, Stored},
    error::Result,
    place::{NOT_ARCHIVED, Place},
};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TagSearch {
    /// Text the tag name must contain, whatever the case.
    q: Option<String>,
}

#[derive(Clone)]
pub struct TagService {
    tags: Service<Tag>,
    places: Service<Place>,
}

impl TagService {
    pub const fn new(tags: Service<Tag>, places: Service<Place>) -> Self {
        Self { tags, places }
    }

    #[tracing::instrument(name = "tag.search", skip_all)]
    pub async fn search(&self, search: TagSearch) -> Result<Vec<Stored<TagUsage>>> {
        let named: Vec<_> = search.q.map(|text| Condition::Contains("name", text)).into_iter().collect();
        let places = self.places.search(&[NOT_ARCHIVED]).await?;
        Ok(self.tags.search(&named).await?.into_iter().map(|tag| usage(tag, &places)).collect())
    }

    #[tracing::instrument(name = "tag.find", skip_all)]
    pub async fn find(&self, id: Uuid) -> Result<Stored<TagUsage>> {
        let tag = self.tags.find(id).await?;
        let tagged = Condition::Includes("tags", json!(tag.value.name));
        Ok(usage(tag, &self.places.search(&[NOT_ARCHIVED, tagged]).await?))
    }

    #[tracing::instrument(name = "tag.create", skip_all)]
    pub async fn create(&self, tag: Tag) -> Result<Stored<TagUsage>> {
        Ok(usage(self.tags.create(tag).await?, &[]))
    }
}

fn usage(tag: Stored<Tag>, places: &[Stored<Place>]) -> Stored<TagUsage> {
    let places = places.iter().filter(|place| place.value.tags.contains(&tag.value.name)).count();
    Stored { id: tag.id, value: TagUsage { tag: tag.value, places } }
}
