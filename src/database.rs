use crate::configuration::DatabaseSettings;
use sqlx::{PgPool, postgres::PgPoolOptions};

pub fn get_connection_pool(configuration: &DatabaseSettings) -> PgPool {
    let max_connections = configuration.max_connections.unwrap_or(10);
    let timeout = configuration.timeout_seconds.unwrap_or(2);

    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(timeout))
        .connect_lazy_with(configuration.with_db())
}
