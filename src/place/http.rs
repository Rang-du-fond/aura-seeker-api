use authn::AuthUser;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use super::{
    Place, PlaceService, PlaceStatistics,
    service::{PlaceScope, PlaceSearch},
};
use crate::{
    crud::{Embedded, Represent, Stored, collection, collection_path, created, item, item_path},
    file::File,
    http::{ApiResult, HAL_JSON, Hal, PROBLEM_JSON, Problem},
    user::Profile,
};

const STATISTICS_PATH: &str = "/places/statistics";

impl Represent for Place {
    fn relate(place: Hal<Stored<Self>>) -> Hal<Stored<Self>> {
        let author = item_path::<Profile>(place.state.value.author);
        let image = item_path::<File>(place.state.value.image);
        let like = format!("{}/like", item_path::<Self>(place.state.id));
        place.link("author", author).link("image", image).link("like", like)
    }
}

#[utoipa::path(
    get, path = "/places", tag = "places", params(PlaceSearch, PlaceScope),
    summary = "Search places that are not archived, nearest first when a radius is given; needs no token",
    responses((status = 200, body = Hal<Embedded<Place>>, content_type = HAL_JSON)),
)]
async fn search(
    State(places): State<PlaceService>,
    Query(search): Query<PlaceSearch>,
) -> ApiResult<Hal<Embedded<Place>>> {
    let search_template =
        format!("{}{{?title,tag,author,liked_by,after,before,latitude,longitude,radius}}", collection_path::<Place>());
    Ok(collection(places.search(search).await?)
        .template("search", search_template)
        .template("statistics", format!("{STATISTICS_PATH}{{?author,after,before}}")))
}

#[utoipa::path(
    get, path = STATISTICS_PATH, tag = "places", params(PlaceScope),
    summary = "Ready-to-display totals for places that are not archived (places, added in the last 30 days, tags used, contributors), plus counts per tag and per creation day; needs no token",
    responses((status = 200, body = Hal<PlaceStatistics>, content_type = HAL_JSON)),
)]
async fn statistics(
    State(places): State<PlaceService>,
    Query(scope): Query<PlaceScope>,
) -> ApiResult<Hal<PlaceStatistics>> {
    let statistics = places.statistics(scope).await?;
    Ok(Hal::new(statistics, STATISTICS_PATH.into()).link("collection", collection_path::<Place>()))
}

#[utoipa::path(
    get, path = "/places/{id}", tag = "places", params(("id" = Uuid, Path)),
    summary = "Read a place, archived or not; needs no token",
    responses(
        (status = 200, body = Hal<Stored<Place>>, content_type = HAL_JSON),
        (status = 404, body = Problem, content_type = PROBLEM_JSON),
    ),
)]
async fn read(State(places): State<PlaceService>, Path(id): Path<Uuid>) -> ApiResult<Hal<Stored<Place>>> {
    Ok(item(places.find(id).await?))
}

#[utoipa::path(
    post, path = "/places", tag = "places", request_body = Place,
    summary = "Create a place authored by the authenticated user; tags are names, unknown ones are created",
    responses(
        (status = 201, body = Hal<Stored<Place>>, content_type = HAL_JSON),
        (status = 409, body = Problem, content_type = PROBLEM_JSON, description = "Unknown image"),
        (status = 422, body = Problem, content_type = PROBLEM_JSON),
    ),
)]
async fn create(
    State(places): State<PlaceService>,
    AuthUser(claims): AuthUser,
    Json(place): Json<Place>,
) -> ApiResult<impl IntoResponse> {
    Ok(created(places.create(claims.sub, place).await?))
}

#[utoipa::path(
    put, path = "/places/{id}", tag = "places", params(("id" = Uuid, Path)), request_body = Place,
    summary = "Replace a place; only its author may. Unknown tags are created",
    responses(
        (status = 200, body = Hal<Stored<Place>>, content_type = HAL_JSON),
        (status = 403, body = Problem, content_type = PROBLEM_JSON),
        (status = 404, body = Problem, content_type = PROBLEM_JSON),
        (status = 409, body = Problem, content_type = PROBLEM_JSON, description = "Unknown image"),
        (status = 422, body = Problem, content_type = PROBLEM_JSON),
    ),
)]
async fn replace(
    State(places): State<PlaceService>,
    AuthUser(claims): AuthUser,
    Path(id): Path<Uuid>,
    Json(place): Json<Place>,
) -> ApiResult<Hal<Stored<Place>>> {
    Ok(item(places.replace(claims.sub, id, place).await?))
}

#[utoipa::path(
    delete, path = "/places/{id}", tag = "places", params(("id" = Uuid, Path)),
    summary = "Archive a place; only its author may. The place is kept and stays readable by id",
    responses(
        (status = 204, description = "Archived"),
        (status = 403, body = Problem, content_type = PROBLEM_JSON),
        (status = 404, body = Problem, content_type = PROBLEM_JSON),
    ),
)]
async fn archive(
    State(places): State<PlaceService>,
    AuthUser(claims): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    places.archive(claims.sub, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    put, path = "/places/{id}/like", tag = "places", params(("id" = Uuid, Path)),
    summary = "Like a place as the authenticated user; authors cannot like their own places",
    responses(
        (status = 204, description = "Liked"),
        (status = 403, body = Problem, content_type = PROBLEM_JSON),
        (status = 404, body = Problem, content_type = PROBLEM_JSON),
    ),
)]
async fn like(
    State(places): State<PlaceService>,
    AuthUser(claims): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    places.like(claims.sub, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete, path = "/places/{id}/like", tag = "places", params(("id" = Uuid, Path)),
    summary = "Remove the authenticated user's like from a place",
    responses((status = 204, description = "Not liked any more")),
)]
async fn unlike(
    State(places): State<PlaceService>,
    AuthUser(claims): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    places.unlike(claims.sub, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router(places: PlaceService) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create))
        .routes(routes!(replace, archive))
        .routes(routes!(like, unlike))
        .with_state(places)
}

pub fn public_router(places: PlaceService) -> OpenApiRouter {
    OpenApiRouter::new().routes(routes!(search)).routes(routes!(statistics)).routes(routes!(read)).with_state(places)
}
