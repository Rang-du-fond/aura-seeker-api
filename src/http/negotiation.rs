use axum::{
    extract::Request,
    http::{
        HeaderValue,
        header::{ACCEPT, CONTENT_TYPE, VARY},
    },
    middleware::Next,
    response::Response,
};
use serde_json::Value;

use super::HAL_JSON;

type Encoder = fn(&Value) -> String;

type Format = (&'static str, Encoder);

const DEFAULT_FORMAT: Format = (HAL_JSON, json);
const FORMATS: &[Format] = &[DEFAULT_FORMAT, ("application/json", json)];

fn json(document: &Value) -> String {
    document.to_string()
}

#[derive(Clone)]
pub struct Document(pub Value);

pub async fn negotiate(request: Request, next: Next) -> Response {
    let accept = request.headers().get(ACCEPT).and_then(|value| value.to_str().ok()).unwrap_or_default().to_owned();
    let mut response = next.run(request).await;
    if let Some(Document(document)) = response.extensions_mut().remove() {
        let (media_type, encode) = preferred(&accept);
        *response.body_mut() = encode(&document).into();
        response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static(media_type));
        response.headers_mut().insert(VARY, HeaderValue::from_static("accept"));
    }
    response
}

fn preferred(accept: &str) -> &'static Format {
    let mut ranges: Vec<_> = accept.split(',').map(weighted).collect();
    ranges.sort_by(|(_, left), (_, right)| right.total_cmp(left));
    ranges
        .iter()
        .find_map(|(range, _)| FORMATS.iter().find(|(offered, _)| accepts(range, offered)))
        .unwrap_or(&DEFAULT_FORMAT)
}

fn weighted(range: &str) -> (&str, f32) {
    let mut parameters = range.split(';').map(str::trim);
    let media_type = parameters.next().unwrap_or_default();
    (media_type, parameters.find_map(|parameter| parameter.strip_prefix("q=")?.parse().ok()).unwrap_or(1.0))
}

fn accepts(range: &str, offered: &str) -> bool {
    let family = range.strip_suffix('*').filter(|family| family.ends_with('/'));
    range == offered || range == "*/*" || family.is_some_and(|family| offered.starts_with(family))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn negotiated(accept: &str) -> &'static str {
        preferred(accept).0
    }

    #[test]
    fn hal_is_the_default_format() {
        assert_eq!(negotiated(""), HAL_JSON);
        assert_eq!(negotiated("*/*"), HAL_JSON);
        assert_eq!(negotiated("text/csv"), HAL_JSON);
    }

    #[test]
    fn the_accept_header_selects_a_supported_format_by_weight() {
        assert_eq!(negotiated("application/json"), "application/json");
        assert_eq!(negotiated("application/*"), HAL_JSON);
        assert_eq!(negotiated("text/csv, application/json;q=0.5, application/hal+json;q=0.9"), HAL_JSON);
        assert_eq!(negotiated("application/hal+json;q=0.1, application/json"), "application/json");
    }
}
