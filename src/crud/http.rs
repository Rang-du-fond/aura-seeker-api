use std::{collections::BTreeMap, marker::PhantomData};

use axum::{
    extract::{Path, State},
    http::{StatusCode, header::LOCATION},
    response::IntoResponse,
    routing::get,
};
use serde::Serialize;
use utoipa::{
    ToSchema,
    openapi::{
        ComponentsBuilder, Content, HttpMethod, OpenApi, OpenApiBuilder, Paths, Ref, Required, ResponseBuilder,
        path::{OperationBuilder, ParameterBuilder, ParameterIn},
        schema::{KnownFormat, ObjectBuilder, SchemaFormat, Type},
    },
};
use utoipa_axum::router::OpenApiRouter;
use uuid::Uuid;

use super::{Resource, Service, Stored};
use crate::http::{ApiResult, HAL_JSON, Hal, PROBLEM_JSON, Problem};

pub trait Represent: Resource + ToSchema {
    fn relate(representation: Hal<Stored<Self>>) -> Hal<Stored<Self>> {
        representation
    }
}

#[derive(Serialize, ToSchema)]
pub struct Embedded<T: Represent> {
    #[serde(rename = "_embedded")]
    embedded: BTreeMap<&'static str, Vec<Hal<Stored<T>>>>,
}

pub fn collection_path<T: Resource>() -> String {
    format!("/{}", T::COLLECTION)
}

pub fn item_path<T: Resource>(id: impl std::fmt::Display) -> String {
    format!("/{}/{id}", T::COLLECTION)
}

pub fn item<T: Represent>(stored: Stored<T>) -> Hal<Stored<T>> {
    let href = item_path::<T>(stored.id);
    T::relate(Hal::new(stored, href).link("collection", collection_path::<T>()))
}

pub fn collection<T: Represent>(items: Vec<Stored<T>>) -> Hal<Embedded<T>> {
    let embedded = BTreeMap::from([(T::COLLECTION, items.into_iter().map(item).collect())]);
    Hal::new(Embedded { embedded }, collection_path::<T>())
}

pub fn created<T: Represent>(stored: Stored<T>) -> impl IntoResponse {
    (StatusCode::CREATED, [(LOCATION, item_path::<T>(stored.id))], item(stored))
}

async fn list<T: Represent>(State(service): State<Service<T>>) -> ApiResult<Hal<Embedded<T>>> {
    Ok(collection(service.search(&[]).await?))
}

async fn read<T: Represent>(State(service): State<Service<T>>, Path(id): Path<Uuid>) -> ApiResult<Hal<Stored<T>>> {
    Ok(item(service.find(id).await?))
}

pub fn readable<T: Represent>() -> OpenApiRouter<Service<T>> {
    let (collection, item) = (collection_path::<T>(), item_path::<T>("{id}"));
    let documentation = documentation::<T>([
        (&collection, HttpMethod::Get, operation::<T>("List").answering(StatusCode::OK, COLLECTION)),
        (&item, HttpMethod::Get, operation::<T>("Read one of").identified().answering(StatusCode::OK, ITEM)),
    ]);
    OpenApiRouter::with_openapi(documentation).route(&collection, get(list::<T>)).route(&item, get(read::<T>))
}

const ITEM: &str = "Hal_Stored";
const COLLECTION: &str = "Hal_Embedded";

#[derive(ToSchema)]
struct Representations<T: Represent> {
    _item: Hal<Stored<T>>,
    _collection: Hal<Embedded<T>>,
    _problem: Problem,
}

fn documentation<T: Represent>(
    operations: impl IntoIterator<Item = (impl AsRef<str>, HttpMethod, Operation<T>)>,
) -> OpenApi {
    let mut schemas = vec![(T::name().into(), T::schema())];
    Representations::<T>::schemas(&mut schemas);
    let mut paths = Paths::new();
    for (path, method, operation) in operations {
        paths.add_path_operation(path, vec![method], operation.0.build());
    }
    OpenApiBuilder::new()
        .paths(paths)
        .components(Some(ComponentsBuilder::new().schemas_from_iter(schemas).build()))
        .build()
}

fn reference(schema: impl Into<String>) -> Content {
    Content::new(Some(Ref::from_schema_name(schema)))
}

fn operation<T: Represent>(action: &str) -> Operation<T> {
    let problem =
        ResponseBuilder::new().description("Problem details").content(PROBLEM_JSON, reference(Problem::name()));
    let operation = OperationBuilder::new().tag(T::COLLECTION).summary(Some(format!("{action} {}", T::COLLECTION)));
    Operation(operation.response("default", problem), PhantomData)
}

struct Operation<T>(OperationBuilder, PhantomData<T>);

impl<T: Represent> Operation<T> {
    fn identified(self) -> Self {
        let id = ParameterBuilder::new().name("id").parameter_in(ParameterIn::Path).required(Required::True);
        let uuid =
            ObjectBuilder::new().schema_type(Type::String).format(Some(SchemaFormat::KnownFormat(KnownFormat::Uuid)));
        Self(self.0.parameter(id.schema(Some(uuid))), PhantomData)
    }

    fn answering(self, status: StatusCode, representation: &str) -> Self {
        let body = reference(format!("{representation}_{}", T::name()));
        let response = ResponseBuilder::new().description(status.canonical_reason().unwrap_or_default());
        Self(self.0.response(status.as_str(), response.content(HAL_JSON, body)), PhantomData)
    }
}
