mod error;
mod hal;
pub mod health;
mod negotiation;

use authn::{AuthUser, SECURITY_SCHEME};
use axum::{Json, Router, middleware, routing::get};
use utoipa::openapi::{InfoBuilder, ResponseBuilder, security::SecurityRequirement};
use utoipa_axum::router::OpenApiRouter;

pub use self::{
    error::{ApiResult, PROBLEM_JSON, Problem},
    hal::{HAL_JSON, Hal},
};
use crate::crud::Resource;

const OPENAPI_PATH: &str = "/openapi.json";

#[derive(Default)]
pub struct Api {
    router: OpenApiRouter,
    collections: Vec<&'static str>,
}

impl Api {
    pub fn mount<T: Resource>(mut self, router: OpenApiRouter) -> Self {
        self.collections.push(T::COLLECTION);
        self.merge(protected(router))
    }

    pub fn merge(mut self, router: OpenApiRouter) -> Self {
        self.router = self.router.merge(router);
        self
    }

    pub fn into_router(self) -> Router {
        let index = self
            .collections
            .into_iter()
            .fold(Hal::new((), "/".into()).link("service-desc", OPENAPI_PATH.into()), |index, name| {
                index.link(name, format!("/{name}"))
            });
        let (router, mut openapi) = self.router.split_for_parts();
        openapi.info = InfoBuilder::new().title("Aura Seeker API").version(env!("CARGO_PKG_VERSION")).build();
        router
            .route("/", get(move || std::future::ready(index.clone())))
            .route(OPENAPI_PATH, get(move || std::future::ready(Json(openapi.clone()))))
            .layer(middleware::from_fn(negotiation::negotiate))
    }
}

fn protected(mut router: OpenApiRouter) -> OpenApiRouter {
    let unauthorized = ResponseBuilder::new().description("Missing or invalid access token").build();
    let items = router.get_openapi_mut().paths.paths.values_mut();
    for operation in items.flat_map(|item| [&mut item.get, &mut item.post, &mut item.put, &mut item.delete]).flatten() {
        operation.security = Some(vec![SecurityRequirement::new(SECURITY_SCHEME, Vec::<String>::new())]);
        operation.responses.responses.insert("401".into(), unauthorized.clone().into());
    }
    router.layer(middleware::from_extractor::<AuthUser>())
}
