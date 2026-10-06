use std::{net::SocketAddr, time::Instant};

use axum::{
    extract::{ConnectInfo, MatchedPath, Request},
    http::{HeaderMap, HeaderValue, StatusCode, header::USER_AGENT},
    middleware::Next,
    response::Response,
};
use opentelemetry::{KeyValue, global, propagation::Extractor};
use tracing::{Instrument, Span, field::Empty};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

pub const REQUEST_ID_HEADER: &str = "x-request-id";
const UNMATCHED_ROUTE: &str = "unmatched";

struct Headers<'request>(&'request HeaderMap);

impl Extractor for Headers<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(axum::http::HeaderName::as_str).collect()
    }
}

pub async fn trace_requests(request: Request, next: Next) -> Response {
    let (started, request_id) = (Instant::now(), Uuid::now_v7().to_string());
    let span = request_span(&request, &request_id);
    let mut response = next.run(request).instrument(span.clone()).await;
    if let Ok(header) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, header);
    }
    span.record("http.response.status_code", response.status().as_u16());
    let _entered = span.enter();
    log_completion(response.status(), started.elapsed().as_millis());
    response
}

fn request_span(request: &Request, request_id: &str) -> Span {
    let route = request.extensions().get::<MatchedPath>().map_or(UNMATCHED_ROUTE, MatchedPath::as_str);
    let span = tracing::info_span!(
        "http.request",
        otel.name = format!("{} {route}", request.method()),
        otel.kind = "server",
        otel.status_code = Empty,
        url.path = request.uri().path(),
        request.id = request_id,
        http.response.status_code = Empty,
        user.id = Empty,
    );
    let parent = global::get_text_map_propagator(|propagator| propagator.extract(&Headers(request.headers())));
    span.set_parent(parent).ok();
    for attribute in request_attributes(request, route) {
        span.set_attribute(attribute.key, attribute.value);
    }
    span
}

fn request_attributes(request: &Request, route: &str) -> Vec<KeyValue> {
    let address =
        request.extensions().get::<ConnectInfo<SocketAddr>>().map(|ConnectInfo(address)| address.ip().to_string());
    let user_agent = request.headers().get(USER_AGENT).and_then(|value| value.to_str().ok()).map(ToOwned::to_owned);
    let query = request.uri().query().map(ToOwned::to_owned);
    let named = [
        ("http.request.method", Some(request.method().to_string())),
        ("http.route", Some(route.to_owned())),
        ("client.address", address),
        ("user_agent.original", user_agent),
        ("url.query", query),
    ];
    named.into_iter().filter_map(|(name, value)| Some(KeyValue::new(name, value?))).collect()
}

fn log_completion(status: StatusCode, latency_ms: u128) {
    if status.is_server_error() {
        Span::current().record("otel.status_code", "ERROR");
        log_failure(latency_ms);
    } else {
        log_success(latency_ms);
    }
}

fn log_success(latency_ms: u128) {
    tracing::info!(latency_ms, "request completed");
}

fn log_failure(latency_ms: u128) {
    tracing::error!(latency_ms, "request failed");
}
