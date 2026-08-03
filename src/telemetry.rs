use crate::configuration::{LogFormat, LoggingSettings, TelemetryProtocol, TelemetrySettings};
use opentelemetry::{KeyValue, global, trace::TracerProvider};
use opentelemetry_otlp::{
    SpanExporter, WithExportConfig, WithTonicConfig, tonic_types::transport::ClientTlsConfig,
};
use opentelemetry_sdk::{
    Resource,
    propagation::TraceContextPropagator,
    trace::{self, SdkTracerProvider, Tracer},
};
use tokio::task::JoinHandle;
use tracing::{Subscriber, subscriber::set_global_default};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_log::LogTracer;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{
    fmt::{self, MakeWriter, format::FmtSpan},
    registry::LookupSpan,
    {EnvFilter, Registry, layer::SubscriberExt},
};

pub fn init_telemetry<Sink>(
    logging_settings: LoggingSettings,
    telemetry_settings: TelemetrySettings,
    sink: Sink,
) -> Result<(), anyhow::Error>
where
    // This "weird" syntax is a higher-ranked trait bound (HRTB)
    // It basically means that Sink implements the `MakeWriter`
    // trait for all choices of the lifetime parameter `'a`
    // Check out https://doc.rust-lang.org/nomicon/hrtb.html
    // for more details.
    Sink: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    // Redirect all `log`'s events to our subscriber
    LogTracer::init().expect("Failed to set a global tracing logger");

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(logging_settings.level.as_str()));

    // Add OpenTelemetry layer if enabled
    if telemetry_settings.enabled {
        tracing::info!(
            "Initializing OpenTelemetry subscriber with endpoint: {}",
            telemetry_settings.endpoint
        );
        let tracer = init_opentelemetry(&telemetry_settings)?;

        match logging_settings.format {
            LogFormat::Pretty => {
                tracing::info!("Initializing pretty logging subscriber");
                let subscriber =
                    get_pretty_subscriber(env_filter, sink).with(OpenTelemetryLayer::new(tracer));

                set_global_default(subscriber).expect("Failed to set subscriber");
            }
            LogFormat::Bunyan => {
                tracing::info!("Initializing Bunyan logging subscriber");
                let subscriber =
                    get_bunyan_subscriber(env_filter, &telemetry_settings.service_name, sink)
                        .with(OpenTelemetryLayer::new(tracer));

                set_global_default(subscriber).expect("Failed to set subscriber");
            }
        }

        return Ok(());
    }

    match logging_settings.format {
        LogFormat::Pretty => {
            tracing::info!("Initializing pretty logging subscriber");
            let subscriber = get_pretty_subscriber(env_filter, sink);

            set_global_default(subscriber).expect("Failed to set subscriber");
        }
        LogFormat::Bunyan => {
            tracing::info!("Initializing Bunyan logging subscriber");
            let subscriber =
                get_bunyan_subscriber(env_filter, &telemetry_settings.service_name, sink);

            set_global_default(subscriber).expect("Failed to set subscriber");
        }
    }

    Ok(())
}

/// Create a pretty, human-readable subscriber to be used in local development
///
/// This format includes:
/// - Colored output (if supported)
/// - File and line numbers
/// - Thread information
/// - Span events (when entering and exiting)
fn get_pretty_subscriber<Sink>(
    env_filter: EnvFilter,
    sink: Sink,
) -> impl Subscriber + Send + Sync + for<'a> LookupSpan<'a>
where
    Sink: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    Registry::default().with(env_filter).with(
        fmt::layer()
            .with_writer(sink)
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_file(true)
            .with_line_number(true)
            .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
            .pretty(),
    )
}

/// Create a Bunyan-formatted subscriber
///
/// Bunyan format is a structured logging format that includes:
/// - Standard fields (v, name, msg, level, hostname, pid, time)
/// - Source location (file, line)
/// - Contextual information from tracing spans
///
/// # Implementation Notes
///
/// We are using `impl Subscriber` as return type to avoid having to
/// spell out the actual type of the returned subscriber, which is
/// indeed quite complex.
/// We need to explicitly call out that the returned subscriber is
/// `Send` and `Sync` to make it possible to pass it to `init_subscriber`
/// later on.
fn get_bunyan_subscriber<Sink>(
    env_filter: EnvFilter,
    service_name: &str,
    sink: Sink,
) -> impl Subscriber + Send + Sync + for<'a> LookupSpan<'a>
where
    Sink: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let formatting_layer = BunyanFormattingLayer::new(service_name.to_string(), sink);

    Registry::default()
        .with(env_filter)
        .with(JsonStorageLayer)
        .with(formatting_layer)
}

fn init_opentelemetry(settings: &TelemetrySettings) -> Result<Tracer, anyhow::Error> {
    // Set up trace context propagation
    global::set_text_map_propagator(TraceContextPropagator::new());

    // Create resource with service information
    let resource = Resource::builder()
        .with_attributes(vec![
            KeyValue::new("service.name", settings.service_name.clone()),
            KeyValue::new("deployment.environment", settings.environment.clone()),
        ])
        .build();

    // Configure OTLP exporter with gRPC or Http
    let exporter = match &settings.protocol {
        TelemetryProtocol::Grpc => {
            let tls_config = ClientTlsConfig::new()
                .with_native_roots()
                .assume_http2(true);

            SpanExporter::builder()
                .with_tonic()
                .with_endpoint(&settings.endpoint)
                .with_tls_config(tls_config)
                .build()?
        }
        TelemetryProtocol::Http => SpanExporter::builder()
            .with_http()
            .with_endpoint(format!(
                "{}/v1/traces",
                settings.endpoint.trim_end_matches('/')
            ))
            .build()?,
    };

    // Build the tracer provider
    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource.clone())
        .with_sampler(trace::Sampler::AlwaysOn)
        .build();

    // Get a tracer from the provider
    let tracer = tracer_provider.tracer("farms-service");

    // Store the provider globally for shutdown
    global::set_tracer_provider(tracer_provider);

    Ok(tracer)
}

// Just copied trait bounds and signature from `spawn_blocking`
pub fn spawn_blocking_with_tracing<F, R>(f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let current_span = tracing::Span::current();
    // `Span::in_scope` only pushes onto the thread-local span stack; it does
    // nothing to install the subscriber itself. `spawn_blocking` hands off to
    // a fresh OS thread, so without also propagating the dispatcher the span
    // stack is pushed against whatever (possibly no-op) subscriber is default
    // on that thread, and the span context is lost.
    let dispatch = tracing::dispatcher::get_default(|d| d.clone());
    #[allow(
        clippy::disallowed_methods,
        reason = "this helper is the sanctioned wrapper"
    )]
    tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&dispatch, || current_span.in_scope(f))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::LoggingLevel;
    use tracing::Instrument;

    #[test]
    fn test_telemetry_initialization_without_opentelemetry() {
        let logging_settings = LoggingSettings {
            level: LoggingLevel::Info,
            format: LogFormat::Pretty,
        };

        let telemetry_settings = TelemetrySettings {
            enabled: false,
            service_name: "test-service".to_string(),
            endpoint: "http://localhost:4317".to_string(),
            environment: "test".to_string(),
            protocol: TelemetryProtocol::Grpc,
        };

        // This shouldn't panic
        assert!(init_telemetry(logging_settings, telemetry_settings, std::io::stdout).is_ok());
    }

    /// The property the whole helper exists for: work running on the blocking
    /// pool must observe the span that was current when it was spawned.
    ///
    /// Asserted by comparing span ids rather than by scraping log output — a
    /// string check would pass for a span that merely has the same *name*.
    #[tokio::test]
    async fn spawn_blocking_with_tracing_carries_the_callers_span() {
        // A subscriber must be active or `Span::current()` is always disabled
        // and both sides trivially compare equal to None.
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let outer = tracing::info_span!("outer");
        let expected = outer.id();
        assert!(
            expected.is_some(),
            "the test subscriber must actually record spans, or this proves nothing"
        );

        let observed = async {
            spawn_blocking_with_tracing(tracing::Span::current)
                .await
                .expect("blocking task panicked")
        }
        .instrument(outer)
        .await;

        assert_eq!(
            observed.id(),
            expected,
            "blocking work ran outside the caller's span — the trace tree would have a hole in it"
        );
    }

    /// The negative control. Without this, the test above could pass because
    /// `Span::current()` happens to be inherited some other way, and we would
    /// never know the helper was doing anything.
    #[tokio::test]
    async fn bare_spawn_blocking_loses_the_span() {
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());

        let outer = tracing::info_span!("outer");
        let expected = outer.id();

        let observed = async {
            #[allow(
                clippy::disallowed_methods,
                reason = "demonstrating why the lint exists"
            )]
            tokio::task::spawn_blocking(tracing::Span::current)
                .await
                .expect("blocking task panicked")
        }
        .instrument(outer)
        .await;

        assert_ne!(
            observed.id(),
            expected,
            "if this ever passes, tokio started propagating spans and the helper is redundant"
        );
    }
}
