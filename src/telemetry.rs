//! OpenTelemetry integration (behind `otel` feature flag).

use opentelemetry::KeyValue;
use opentelemetry_sdk::trace;
use opentelemetry_sdk::Resource;
use opentelemetry_otlp::WithExportConfig;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::Registry;

/// Initialize OpenTelemetry tracing and return a layer for the subscriber stack.
pub fn init_otel_tracing(
    endpoint: &str,
    service_name: &str,
) -> Result<OpenTelemetryLayer<Registry, opentelemetry_sdk::trace::Tracer>, Box<dyn std::error::Error>> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(Resource::builder().with_attributes(vec![
            KeyValue::new("service.name", service_name.to_string()),
        ]).build())
        .build();

    let tracer = provider.tracer("lazypaw");
    opentelemetry::global::set_tracer_provider(provider);

    Ok(OpenTelemetryLayer::new(tracer))
}
