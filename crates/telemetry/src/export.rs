use std::env;

use opentelemetry::{global, trace::TracerProvider};
use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator, trace::SdkTracerProvider};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::registry::LookupSpan;

use crate::Cause;

const DEFAULT_SERVICE_NAME: &str = "aura-api";
const OTLP: &str = "otlp";
const CONSOLE: &str = "console";

pub struct Exporter {
    destination: String,
    provider: Option<SdkTracerProvider>,
}

impl Exporter {
    pub fn from_environment() -> Result<Self, Cause> {
        let destination = env::var("OTEL_TRACES_EXPORTER").unwrap_or_default();
        let traces =
            SdkTracerProvider::builder().with_resource(Resource::builder().with_service_name(service_name()).build());
        let provider = match destination.as_str() {
            OTLP => Some(
                traces.with_batch_exporter(opentelemetry_otlp::SpanExporter::builder().with_tonic().build()?).build(),
            ),
            CONSOLE => Some(traces.with_simple_exporter(opentelemetry_stdout::SpanExporter::default()).build()),
            _ => None,
        };
        global::set_text_map_propagator(TraceContextPropagator::new());
        Ok(Self { destination, provider })
    }

    #[must_use]
    pub fn layer<S: tracing::Subscriber + for<'span> LookupSpan<'span>>(
        &self,
    ) -> Option<OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>> {
        let provider = self.provider.as_ref()?;
        Some(tracing_opentelemetry::layer().with_tracer(provider.tracer(service_name())))
    }

    pub fn announce(&self) {
        if self.provider.is_some() {
            tracing::info!(
                exporter = self.destination,
                service = service_name(),
                "traces are exported with OpenTelemetry"
            );
        }
    }

    pub fn flush(&self) {
        if let Some(Err(cause)) = self.provider.as_ref().map(SdkTracerProvider::shutdown) {
            tracing::warn!(%cause, "the last traces could not be exported");
        }
    }
}

fn service_name() -> String {
    env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| DEFAULT_SERVICE_NAME.into())
}
