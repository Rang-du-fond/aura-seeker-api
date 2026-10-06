use authn::AuthUser;
use axum::{
    Json,
    extract::{Path, State},
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use super::{Profile, ProfileService};
use crate::{
    crud::{Represent, Stored, collection_path, item, readable},
    http::{ApiResult, HAL_JSON, Hal, PROBLEM_JSON, Problem},
    place::Place,
};

impl Represent for Profile {
    fn relate(profile: Hal<Stored<Self>>) -> Hal<Stored<Self>> {
        let places = format!("{}?author={}", collection_path::<Place>(), profile.state.id);
        let likes = format!("{}?liked_by={}", collection_path::<Place>(), profile.state.id);
        profile.link("places", places).link("likes", likes)
    }
}

#[utoipa::path(
    put, path = "/users/{id}", tag = "users", params(("id" = Uuid, Path)), request_body = Profile,
    summary = "Replace a user's public profile; only that user may",
    responses(
        (status = 200, body = Hal<Stored<Profile>>, content_type = HAL_JSON),
        (status = 403, body = Problem, content_type = PROBLEM_JSON),
        (status = 422, body = Problem, content_type = PROBLEM_JSON),
    ),
)]
async fn replace(
    State(profiles): State<ProfileService>,
    AuthUser(claims): AuthUser,
    Path(id): Path<Uuid>,
    Json(profile): Json<Profile>,
) -> ApiResult<Hal<Stored<Profile>>> {
    Ok(item(profiles.replace(claims.sub, id, profile).await?))
}

pub fn router(profiles: ProfileService) -> OpenApiRouter {
    OpenApiRouter::new().routes(routes!(replace)).with_state(profiles.clone()).merge(readable().with_state(profiles.0))
}
