pub mod configuration;
pub mod domain;
pub mod errors;
pub mod idempotency;
pub mod routes;
pub mod startup;
pub mod telemetry;

pub mod authentication;
mod email_client;
mod rate_limit;
