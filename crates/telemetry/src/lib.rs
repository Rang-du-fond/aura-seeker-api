mod export;
mod request;

use std::env;

use tracing_subscriber::{EnvFilter, Layer, Registry, layer::SubscriberExt, util::SubscriberInitExt};

pub use self::{
    export::Exporter,
    request::{REQUEST_ID_HEADER, trace_requests},
};

const DEFAULT_FILTER: &str = "info,sqlx=warn";
const JSON_FORMAT: &str = "json";

type Cause = Box<dyn std::error::Error + Send + Sync>;

pub fn start() -> Result<Exporter, Cause> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| DEFAULT_FILTER.into());
    let logs: Box<dyn Layer<Registry> + Send + Sync> = match env::var("LOG_FORMAT").as_deref() {
        Ok(JSON_FORMAT) => tracing_subscriber::fmt::layer().json().flatten_event(true).boxed(),
        _ => tracing_subscriber::fmt::layer().boxed(),
    };
    let exporter = Exporter::from_environment()?;
    tracing_subscriber::registry().with(logs).with(exporter.layer()).with(filter).try_init()?;
    exporter.announce();
    Ok(exporter)
}
