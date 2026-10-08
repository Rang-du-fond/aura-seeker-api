use std::time::Duration;

use axum::http::{
    HeaderName, HeaderValue, Method,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, LOCATION},
};
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use crate::error::{Error, Result};

const PREFLIGHT_CACHE: Duration = Duration::from_secs(3600);
const REQUEST_ID: HeaderName = HeaderName::from_static(telemetry::REQUEST_ID_HEADER);
const TRACE_PARENT: HeaderName = HeaderName::from_static("traceparent");

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct HttpSettings {
    pub allowed_origins: Vec<String>,
}

pub fn cross_origin(settings: &HttpSettings) -> Result<CorsLayer> {
    let origins =
        settings.allowed_origins.iter().map(|origin| HeaderValue::from_str(origin).map_err(Error::unexpected));
    Ok(CorsLayer::new()
        .allow_origin(origins.collect::<Result<Vec<_>>>()?)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE, ACCEPT, TRACE_PARENT])
        .expose_headers([LOCATION, REQUEST_ID])
        .max_age(PREFLIGHT_CACHE))
}
