use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
};
use bytes::Bytes;
use utoipa_axum::{
    router::{OpenApiRouter, UtoipaMethodRouterExt},
    routes,
};
use uuid::Uuid;

use super::{File, FileService};
use crate::{
    crud::{Represent, Stored, created, item_path, readable},
    http::{ApiResult, HAL_JSON, Hal, PROBLEM_JSON, Problem},
};

const UPLOAD_LIMIT: usize = 20 * 1024 * 1024;

impl Represent for File {
    fn relate(file: Hal<Stored<Self>>) -> Hal<Stored<Self>> {
        let content = format!("{}/content", item_path::<Self>(file.state.id));
        file.link("content", content)
    }
}

#[utoipa::path(
    post, path = "/files", tag = "files",
    summary = "Upload a file: the raw bytes as the body, its media type as Content-Type (20 MiB at most)",
    request_body(content = Vec<u8>, content_type = "*/*"),
    responses((status = 201, body = Hal<Stored<File>>, content_type = HAL_JSON)),
)]
async fn upload(State(files): State<FileService>, headers: HeaderMap, content: Bytes) -> ApiResult<impl IntoResponse> {
    let media_type = headers.get(CONTENT_TYPE).and_then(|value| value.to_str().ok());
    Ok(created(files.upload(media_type.unwrap_or("application/octet-stream").into(), content).await?))
}

#[utoipa::path(
    get, path = "/files/{id}/content", tag = "files", params(("id" = Uuid, Path)),
    summary = "Download a file's bytes, served with the media type it was uploaded with; needs no token",
    responses(
        (status = 200, body = Vec<u8>, content_type = "*/*"),
        (status = 404, body = Problem, content_type = PROBLEM_JSON),
    ),
)]
async fn download(State(files): State<FileService>, Path(id): Path<Uuid>) -> ApiResult<impl IntoResponse> {
    let (file, content) = files.download(id).await?;
    Ok(([(CONTENT_TYPE, file.media_type)], content))
}

#[utoipa::path(
    delete, path = "/files/{id}", tag = "files", params(("id" = Uuid, Path)),
    summary = "Delete a file and its bytes",
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, body = Problem, content_type = PROBLEM_JSON),
        (status = 409, body = Problem, content_type = PROBLEM_JSON, description = "A place still uses this file"),
    ),
)]
async fn remove(State(files): State<FileService>, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    files.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router(files: FileService) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(upload).layer(DefaultBodyLimit::max(UPLOAD_LIMIT)))
        .routes(routes!(remove))
        .with_state(files.clone())
        .merge(readable().with_state(files.metadata))
}

pub fn public_router(files: FileService) -> OpenApiRouter {
    OpenApiRouter::new().routes(routes!(download)).with_state(files)
}
