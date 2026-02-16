//! OpenTelemetry integration (behind `otel` feature flag).

use opentelemetry::trace::TracerProvider;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::registry::LookupSpan;

/// Initialize OpenTelemetry tracing and return a layer for the subscriber stack.
pub fn init_otel_tracing<S>(
    endpoint: &str,
    service_name: &str,
) -> Result<OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>, Box<dyn std::error::Error>>
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;

    let resource = Resource::new(vec![KeyValue::new(
        "service.name",
        service_name.to_string(),
    )]);

    let provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer("lazypaw");
    opentelemetry::global::set_tracer_provider(provider);

    Ok(OpenTelemetryLayer::new(tracer))
}
