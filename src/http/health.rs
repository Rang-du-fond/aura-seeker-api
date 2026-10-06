use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::crud::SqlRepository;

#[derive(Serialize, ToSchema)]
struct Health {
    status: &'static str,
}

#[utoipa::path(
    get, path = "/health", tag = "health",
    summary = "Tell whether the server is up and can reach its database; needs no token",
    responses((status = 200, body = Health, description = "ok"), (status = 503, body = Health, description = "unavailable")),
)]
async fn health(State(database): State<SqlRepository>) -> (StatusCode, Json<Health>) {
    if database.is_reachable().await {
        (StatusCode::OK, Json(Health { status: "ok" }))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(Health { status: "unavailable" }))
    }
}

pub fn router(database: SqlRepository) -> OpenApiRouter {
    OpenApiRouter::new().routes(routes!(health)).with_state(database)
}
