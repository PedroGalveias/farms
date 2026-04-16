//! src/main.rs

use farms::{
    configuration::get_configuration, idempotency::run_expiry_worker_until_stopped,
    startup::Application, telemetry::init_telemetry,
};
use std::fmt::{Debug, Display};
use tokio::task::JoinError;

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
        "Starting {} on {}:{}",
        configuration.telemetry.service_name,
        configuration.application.host,
        configuration.application.port,
    );
    tracing::info!(
        environment = %configuration.telemetry.environment,
        log_format = ?configuration.logging.format,
        log_level = ?configuration.logging.level,
        telemetry_enabled = %configuration.telemetry.enabled,
        "Configuration loaded.",
    );

    let application = Application::build(configuration.clone())
        .await
        .expect("Failed to build application.");
    let application_task = tokio::spawn(application.run_until_stopped());
    let idempotency_cleanup_worker_task =
        tokio::spawn(run_expiry_worker_until_stopped(configuration));

    tokio::select! {
        o = application_task => report_exit("API", o),
        o = idempotency_cleanup_worker_task => report_exit("Idempotency cleanup worker", o),
    }

    Ok(())
}

fn report_exit(task_name: &str, outcome: Result<Result<(), impl Debug + Display>, JoinError>) {
    match outcome {
        Ok(Ok(())) => {
            tracing::info!("{} exited successfully", task_name);
        }
        Ok(Err(e)) => {
            tracing::error!(
                error.cause_chain = ?e,
                error.message = %e,
                "{} failed",
                task_name
            );
        }
        Err(e) => {
            tracing::error!(
                error.cause_chain = ?e,
                error.message = %e,
                "{}' task failed to complete",
                task_name
            );
        }
    }
}
