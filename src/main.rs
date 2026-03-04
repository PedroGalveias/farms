//! src/main.rs

use farms::{configuration::get_configuration, startup::Application, telemetry::init_telemetry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let configuration = get_configuration().expect("Failed to read configuration.");

    // Init telemetry
    init_telemetry(
        configuration.logging.clone(),
        configuration.telemetry.clone(),
        std::io::stdout,
    )
    .expect("Failed to initialize telemetry.");

    // Log startup information
    tracing::info!(
        "Starting {} on {}",
        configuration.telemetry.service_name,
        configuration.application.base_url,
    );
    tracing::info!(
        environment = %configuration.telemetry.environment,
        log_format = ?configuration.logging.format,
        log_level = ?configuration.logging.level,
        telemetry_enabled = %configuration.telemetry.enabled,
        "Configuration loaded.",
    );

    let application = Application::build(configuration)
        .await
        .expect("Failed to build application.");
    let result = application.run_until_stopped().await;

    result.expect("Failed to shutdown application.");

    Ok(())
}
