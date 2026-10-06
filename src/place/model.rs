use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::crud::{Condition, Record};

const EARTH_RADIUS: f64 = 6_371_000.0;
const RECENT_DAYS: i64 = 30;
pub const LIKES: &str = "liked_by";
pub const NOT_ARCHIVED: Condition = Condition::Missing("archived_at");

#[derive(Serialize, Deserialize, Validate, ToSchema)]
pub struct Place {
    #[validate(length(min = 1))]
    pub title: String,
    pub description: Option<String>,
    #[validate(range(min = -90.0, max = 90.0))]
    pub latitude: f64,
    #[validate(range(min = -180.0, max = 180.0))]
    pub longitude: f64,
    pub image: Uuid,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    #[schema(read_only)]
    pub author: Uuid,
    #[serde(default)]
    #[schema(read_only)]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    #[schema(read_only)]
    pub archived_at: Option<DateTime<Utc>>,
}

impl Record for Place {
    const COLLECTION: &'static str = "places";
    const LISTS: &'static [&'static str] = &["tags"];
}

#[derive(Serialize, ToSchema)]
pub struct PlaceStatistics {
    total: usize,
    added_last_30_days: usize,
    tags_used: usize,
    contributors: usize,
    per_tag: BTreeMap<String, usize>,
    per_day: BTreeMap<NaiveDate, usize>,
}

impl PlaceStatistics {
    pub fn summarizing(places: &[Place], now: DateTime<Utc>) -> Self {
        let recent_since = now - Duration::days(RECENT_DAYS);
        let per_tag = counted(places.iter().flat_map(|place| place.tags.iter().cloned()));
        Self {
            total: places.len(),
            added_last_30_days: places.iter().filter(|place| place.created_at > recent_since).count(),
            tags_used: per_tag.len(),
            contributors: places.iter().map(|place| place.author).collect::<BTreeSet<_>>().len(),
            per_tag,
            per_day: counted(places.iter().map(|place| place.created_at.date_naive())),
        }
    }
}

fn counted<K: Ord>(keys: impl Iterator<Item = K>) -> BTreeMap<K, usize> {
    keys.fold(BTreeMap::new(), |mut counts, key| {
        *counts.entry(key).or_default() += 1;
        counts
    })
}

pub struct Area {
    pub latitude: f64,
    pub longitude: f64,
    pub radius: f64,
}

impl Area {
    pub fn haversine_distance_to(&self, place: &Place) -> f64 {
        let (from, to) = (self.latitude.to_radians(), place.latitude.to_radians());
        let half_longitude_gap = (place.longitude - self.longitude).to_radians() / 2.0;
        let half_chord =
            (from.cos() * to.cos()).mul_add(half_longitude_gap.sin().powi(2), ((to - from) / 2.0).sin().powi(2));
        2.0 * EARTH_RADIUS * half_chord.sqrt().asin()
    }

    pub fn bounding_box(&self) -> Vec<Condition> {
        let reach = self.radius / EARTH_RADIUS;
        let (south, north) = (self.latitude - reach.to_degrees(), self.latitude + reach.to_degrees());
        let stretch = (reach.sin() / self.latitude.to_radians().cos()).asin().to_degrees();
        let (west, east) = (self.longitude - stretch, self.longitude + stretch);
        let latitudes =
            [Condition::GreaterThan("latitude", json!(south)), Condition::LessThan("latitude", json!(north))];
        let longitudes =
            [Condition::GreaterThan("longitude", json!(west)), Condition::LessThan("longitude", json!(east))];
        let within_antimeridian = west >= -180.0 && east <= 180.0;
        latitudes.into_iter().chain(longitudes.into_iter().filter(|_| within_antimeridian)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RENNES: (f64, f64) = (48.1173, -1.6778);
    const NANTES: (f64, f64) = (47.2184, -1.5536);

    fn place_at((latitude, longitude): (f64, f64), tags: &[&str], author: u128, created_at: DateTime<Utc>) -> Place {
        Place {
            title: "Banc".into(),
            description: None,
            latitude,
            longitude,
            image: Uuid::nil(),
            tags: tags.iter().map(ToString::to_string).collect(),
            author: Uuid::from_u128(author),
            created_at,
            archived_at: None,
        }
    }

    #[test]
    fn haversine_distance_between_rennes_and_nantes_is_about_100_km() {
        let around_rennes = Area { latitude: RENNES.0, longitude: RENNES.1, radius: 0.0 };
        let distance = around_rennes.haversine_distance_to(&place_at(NANTES, &[], 1, Utc::now()));
        assert!((distance - 100_400.0).abs() < 1_000.0, "{distance}");
    }

    #[test]
    fn bounding_box_drops_the_longitude_bounds_across_the_antimeridian() {
        let inland = Area { latitude: RENNES.0, longitude: RENNES.1, radius: 10_000.0 };
        let near_antimeridian = Area { latitude: -17.0, longitude: 179.95, radius: 50_000.0 };
        assert_eq!(inland.bounding_box().len(), 4);
        assert_eq!(near_antimeridian.bounding_box().len(), 2);
    }

    #[test]
    fn statistics_count_recent_places_tags_and_contributors() {
        let now = Utc::now();
        let places = [
            place_at(RENNES, &["banc", "rennes"], 1, now),
            place_at(RENNES, &["banc"], 1, now - Duration::days(45)),
            place_at(NANTES, &["nantes"], 2, now - Duration::days(2)),
        ];
        let statistics = PlaceStatistics::summarizing(&places, now);
        assert_eq!(
            (statistics.total, statistics.added_last_30_days, statistics.tags_used, statistics.contributors),
            (3, 2, 3, 2)
        );
        assert_eq!(statistics.per_tag.get("banc"), Some(&2));
    }
}
