use std::collections::BTreeMap;

use axum::{
    Extension,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

use super::{error::ApiError, negotiation::Document};
use crate::error::Error;

pub const HAL_JSON: &str = "application/hal+json";

#[derive(Clone, Serialize, ToSchema)]
struct Link {
    href: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    templated: bool,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct Hal<S> {
    #[serde(flatten)]
    pub state: S,
    #[serde(rename = "_links")]
    links: BTreeMap<&'static str, Link>,
}

impl<S> Hal<S> {
    pub fn new(state: S, href: String) -> Self {
        Self { state, links: BTreeMap::new() }.link("self", href)
    }

    pub fn link(mut self, relation: &'static str, href: String) -> Self {
        self.links.insert(relation, Link { href, templated: false });
        self
    }

    pub fn template(mut self, relation: &'static str, href: String) -> Self {
        self.links.insert(relation, Link { href, templated: true });
        self
    }
}

impl<S: Serialize> IntoResponse for Hal<S> {
    fn into_response(self) -> Response {
        match serde_json::to_value(self) {
            Ok(document) => Extension(Document(document)).into_response(),
            Err(cause) => ApiError::from(Error::unexpected(cause)).into_response(),
        }
    }
}
