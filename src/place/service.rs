use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use utoipa::IntoParams;
use uuid::Uuid;

use super::{
    NOT_ARCHIVED, Place, PlaceStatistics,
    model::{Area, LIKES},
};
use crate::{
    crud::{Condition, Service, Stored},
    error::{Error, Result},
    tag::Tag,
};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PlaceScope {
    author: Option<Uuid>,
    after: Option<DateTime<Utc>>,
    before: Option<DateTime<Utc>>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PlaceSearch {
    title: Option<String>,
    tag: Option<String>,
    liked_by: Option<Uuid>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    /// Metres around `latitude` and `longitude`.
    radius: Option<f64>,
    #[serde(flatten)]
    #[param(ignore)]
    scope: PlaceScope,
}

impl PlaceSearch {
    fn area(&self) -> Option<Area> {
        let (latitude, longitude, radius) = (self.latitude?, self.longitude?, self.radius?);
        Some(Area { latitude, longitude, radius })
    }

    fn conditions(self) -> Vec<Condition> {
        let area = self.area().map(|area| area.bounding_box()).unwrap_or_default();
        let title = self.title.map(|title| Condition::Contains("title", title));
        let tag = self.tag.map(|tag| Condition::Includes("tags", json!(tag)));
        let liked = self.liked_by.map(|user| Condition::Includes(LIKES, json!(user)));
        self.scope.conditions().into_iter().chain(area).chain(title).chain(tag).chain(liked).collect()
    }
}

impl PlaceScope {
    fn conditions(self) -> Vec<Condition> {
        [
            Some(NOT_ARCHIVED),
            self.author.map(|author| Condition::Equals("author", json!(author))),
            self.after.map(|after| Condition::GreaterThan("created_at", json!(after))),
            self.before.map(|before| Condition::LessThan("created_at", json!(before))),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[derive(Clone)]
pub struct PlaceService {
    places: Service<Place>,
    tags: Service<Tag>,
}

impl PlaceService {
    pub const fn new(places: Service<Place>, tags: Service<Tag>) -> Self {
        Self { places, tags }
    }

    #[tracing::instrument(name = "place.search", skip_all)]
    pub async fn search(&self, search: PlaceSearch) -> Result<Vec<Stored<Place>>> {
        let area = search.area();
        let mut places = self.places.search(&search.conditions()).await?;
        if let Some(area) = area {
            let distance = |place: &Stored<Place>| area.haversine_distance_to(&place.value);
            places.retain(|place| distance(place) <= area.radius);
            places.sort_by(|left, right| distance(left).total_cmp(&distance(right)));
        }
        Ok(places)
    }

    #[tracing::instrument(name = "place.statistics", skip_all)]
    pub async fn statistics(&self, scope: PlaceScope) -> Result<PlaceStatistics> {
        let places: Vec<_> =
            self.places.search(&scope.conditions()).await?.into_iter().map(|place| place.value).collect();
        Ok(PlaceStatistics::summarizing(&places, Utc::now()))
    }

    #[tracing::instrument(name = "place.find", skip_all)]
    pub async fn find(&self, id: Uuid) -> Result<Stored<Place>> {
        self.places.find(id).await
    }

    #[tracing::instrument(name = "place.create", skip_all)]
    pub async fn create(&self, author: Uuid, place: Place) -> Result<Stored<Place>> {
        let tags = self.existing_tags(place.tags).await?;
        self.places.create(Place { tags, author, created_at: Utc::now(), archived_at: None, ..place }).await
    }

    #[tracing::instrument(name = "place.replace", skip_all)]
    pub async fn replace(&self, author: Uuid, id: Uuid, place: Place) -> Result<Stored<Place>> {
        let Place { created_at, archived_at, .. } = self.authored_by(author, id).await?;
        let tags = self.existing_tags(place.tags).await?;
        self.places.replace(id, Place { tags, author, created_at, archived_at, ..place }).await
    }

    #[tracing::instrument(name = "place.archive", skip_all)]
    pub async fn archive(&self, author: Uuid, id: Uuid) -> Result<()> {
        let place = self.authored_by(author, id).await?;
        let archived_at = place.archived_at.or_else(|| Some(Utc::now()));
        self.places.replace(id, Place { archived_at, ..place }).await.map(drop)
    }

    #[tracing::instrument(name = "place.like", skip_all)]
    pub async fn like(&self, user: Uuid, id: Uuid) -> Result<()> {
        if self.find(id).await?.value.author == user {
            return Err(Error::Forbidden("you cannot like your own place"));
        }
        self.places.add_to_list(id, LIKES, json!(user)).await
    }

    #[tracing::instrument(name = "place.unlike", skip_all)]
    pub async fn unlike(&self, user: Uuid, id: Uuid) -> Result<()> {
        self.places.remove_from_list(id, LIKES, json!(user)).await
    }

    async fn existing_tags(&self, mut names: Vec<String>) -> Result<Vec<String>> {
        names.sort();
        names.dedup();
        for name in &names {
            self.ensure_tag(name).await?;
        }
        Ok(names)
    }

    async fn ensure_tag(&self, name: &str) -> Result<()> {
        if self.tags.search(&[Condition::Equals("name", json!(name))]).await?.is_empty() {
            self.tags.create(Tag { name: name.to_owned() }).await?;
        }
        Ok(())
    }

    async fn authored_by(&self, author: Uuid, id: Uuid) -> Result<Place> {
        Some(self.find(id).await?.value)
            .filter(|place| place.author == author)
            .ok_or(Error::Forbidden("only its author can change a place"))
    }
}
