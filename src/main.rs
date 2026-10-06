mod crud;
mod error;
mod file;
mod http;
mod place;
mod tag;
mod user;

use std::{env, fs, io::ErrorKind, net::SocketAddr};

use authn::Authenticator;
use axum::{Extension, middleware};
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use tokio::{
    net::TcpListener,
    signal::unix::{SignalKind, signal},
};

use crate::{
    crud::{Migration, Service, SqlRepository},
    file::{File, FileService, ObjectStorage},
    http::Api,
    place::{Place, PlaceService},
    tag::{Tag, TagService},
    user::{Profile, ProfileService},
};

static MIGRATIONS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

fn migrations() -> Vec<Migration> {
    let mut migrations: Vec<_> = MIGRATIONS
        .files()
        .filter(|file| file.path().extension().is_some_and(|extension| extension == "sql"))
        .filter_map(|file| Some(Migration { name: file.path().file_stem()?.to_str()?, sql: file.contents_utf8()? }))
        .collect();
    migrations.sort_by_key(|migration| migration.name);
    migrations
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Configuration {
    auth: authn::Settings,
    notifications: notifier::Settings,
}

impl Configuration {
    fn load() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let file = setting("CONFIG_FILE", "config.toml");
        match fs::read_to_string(&file) {
            Ok(content) => Ok(toml::from_str(&content)?),
            Err(cause) if cause.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(cause) => Err(format!("cannot read {file}: {cause}").into()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let telemetry = telemetry::start()?;

    let uploads = format!("file://{}/uploads", env::current_dir()?.display());
    let database_url = setting("DATABASE_URL", "sqlite://aura.db?mode=rwc");
    let database = SqlRepository::connect(&database_url, &migrations()).await?;
    let storage = ObjectStorage::open(&setting("STORAGE_URL", &uploads))?;
    let configuration = Configuration::load()?;
    let notifier = notifier::email_notifier(&configuration.notifications)?;
    let authenticator = Authenticator::new(database.clone(), notifier, configuration.auth)?;
    authenticator.start_cleanup();
    let files = FileService::new(Service::new(database.clone()), storage);

    let tags = Service::<Tag>::new(database.clone());
    let tag_usage = TagService::new(tags.clone(), Service::new(database.clone()));
    let places = PlaceService::new(Service::new(database.clone()), tags);

    let api = Api::default()
        .mount::<Tag>(tag::router(tag_usage.clone()))
        .merge(tag::public_router(tag_usage))
        .mount::<Place>(place::router(places.clone()))
        .merge(place::public_router(places))
        .mount::<Profile>(user::router(ProfileService(Service::new(database.clone()))))
        .mount::<File>(file::router(files.clone()))
        .merge(file::public_router(files))
        .merge(http::health::router(database))
        .merge(authn::router(authenticator.clone()))
        .into_router()
        .layer(Extension(authenticator))
        .layer(middleware::from_fn(telemetry::trace_requests));

    let listener = TcpListener::bind(setting("BIND_ADDRESS", "0.0.0.0:8080")).await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, api.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_requested())
        .await?;
    telemetry.flush();
    Ok(())
}

async fn shutdown_requested() {
    let mut termination = signal(SignalKind::terminate()).ok();
    let terminated = async {
        match &mut termination {
            Some(termination) => termination.recv().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminated => {}
    }
}

fn setting(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.into())
}
