use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use url::form_urlencoded::byte_serialize;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use super::{Tag, TagService, model::TagUsage, service::TagSearch};
use crate::{
    crud::{Embedded, Represent, Stored, collection, collection_path, created, item},
    http::{ApiResult, HAL_JSON, Hal, PROBLEM_JSON, Problem},
    place::Place,
};

impl Represent for TagUsage {
    fn relate(tag: Hal<Stored<Self>>) -> Hal<Stored<Self>> {
        let name: String = byte_serialize(tag.state.value.tag.name.as_bytes()).collect();
        let places = format!("{}?tag={name}", collection_path::<Place>());
        tag.link("places", places)
    }
}

#[utoipa::path(
    get, path = "/tags", tag = "tags", params(TagSearch),
    summary = "List tags, optionally only those whose name contains `q`, each with its number of non-archived places; needs no token",
    responses((status = 200, body = Hal<Embedded<TagUsage>>, content_type = HAL_JSON)),
)]
async fn list(State(tags): State<TagService>, Query(search): Query<TagSearch>) -> ApiResult<Hal<Embedded<TagUsage>>> {
    let template = format!("{}{{?q}}", collection_path::<Tag>());
    Ok(collection(tags.search(search).await?).template("search", template))
}

#[utoipa::path(
    post, path = "/tags", tag = "tags", request_body = Tag,
    summary = "Create a tag",
    responses(
        (status = 201, body = Hal<Stored<TagUsage>>, content_type = HAL_JSON),
        (status = 409, body = Problem, content_type = PROBLEM_JSON, description = "A tag already has this name"),
        (status = 422, body = Problem, content_type = PROBLEM_JSON),
    ),
)]
async fn create(State(tags): State<TagService>, Json(tag): Json<Tag>) -> ApiResult<impl IntoResponse> {
    Ok(created(tags.create(tag).await?))
}

#[utoipa::path(
    get, path = "/tags/{id}", tag = "tags", params(("id" = Uuid, Path)),
    summary = "Read a tag with the number of places that carry it and are not archived; needs no token",
    responses(
        (status = 200, body = Hal<Stored<TagUsage>>, content_type = HAL_JSON),
        (status = 404, body = Problem, content_type = PROBLEM_JSON),
    ),
)]
async fn read(State(tags): State<TagService>, Path(id): Path<Uuid>) -> ApiResult<Hal<Stored<TagUsage>>> {
    Ok(item(tags.find(id).await?))
}

pub fn router(tags: TagService) -> OpenApiRouter {
    OpenApiRouter::new().routes(routes!(create)).with_state(tags)
}

pub fn public_router(tags: TagService) -> OpenApiRouter {
    OpenApiRouter::new().routes(routes!(list)).routes(routes!(read)).with_state(tags)
}
