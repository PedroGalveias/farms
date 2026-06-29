use crate::configuration::{IdempotencyEngine, Settings};
use crate::startup::get_connection_pool;
use sqlx::PgPool;
use std::time::Duration;
use tracing::Span;

#[derive(Debug, PartialEq)]
pub enum ExpiryOutcome {
    RowsDeleted(u64),
    NothingToDelete,
}

pub async fn run_expiry_worker_until_stopped(configuration: Settings) -> Result<(), anyhow::Error> {
    let idempotency_settings = configuration.idempotency;
    match idempotency_settings.engine {
        IdempotencyEngine::Redis | IdempotencyEngine::None => {
            return Ok(());
        }
        _ => {}
    }

    tracing::info!(
        cleanup_worker_run_interval = idempotency_settings.cleanup_worker_run_interval,
        "Expiry worker starting."
    );

    let connection_pool = get_connection_pool(&configuration.database);
    worker_loop(
        connection_pool,
        idempotency_settings.cleanup_worker_run_interval,
    )
    .await
}

async fn worker_loop(pool: PgPool, run_interval: u64) -> Result<(), anyhow::Error> {
    let sleep_duration = Duration::from_mins(run_interval);

    loop {
        match try_to_execute_task(&pool).await {
            Ok(ExpiryOutcome::NothingToDelete) => {
                tracing::debug!("Expiry worker found no expired rows this run.");
            }
            Ok(ExpiryOutcome::RowsDeleted(n)) => {
                tracing::info!(deleted_rows = n, "Expiry worker deleted expired rows.");
            }
            Err(e) => {
                tracing::error!(
                    error.cause_chain = ?e,
                    error.message     = %e,
                    "Expiry worker encountered an error during cleanup."
                );
                // Back off briefly on error so we don't hammer the DB.
                tokio::time::sleep(Duration::from_secs(30)).await;
                continue;
            }
        }

        tracing::debug!(
            cleanup_worker_run_interval = run_interval,
            "Expiry worker sleeping until next scheduled run."
        );
        tokio::time::sleep(sleep_duration).await;
    }
}

#[tracing::instrument(
    skip_all,
    fields(
        deleted_rows = tracing::field::Empty,
    ),
    err
)]
pub async fn try_to_execute_task(pool: &PgPool) -> Result<ExpiryOutcome, anyhow::Error> {
    let deleted = delete_expired_idempotency_rows(pool).await?;

    Span::current().record("deleted_rows", deleted);

    if deleted == 0 {
        Ok(ExpiryOutcome::NothingToDelete)
    } else {
        Ok(ExpiryOutcome::RowsDeleted(deleted))
    }
}

#[tracing::instrument(skip_all)]
async fn delete_expired_idempotency_rows(pool: &PgPool) -> Result<u64, anyhow::Error> {
    let result = sqlx::query!("DELETE FROM idempotency WHERE expire_at < NOW()")
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}
