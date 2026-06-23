use deadpool_redis::{Pool, redis::AsyncTypedCommands};

pub enum RateLimitDecision {
    Allowed,
    Limited,
}

#[derive(thiserror::Error, Debug)]
pub enum RateLimitError {
    #[error("Failed to get a Valkey connection from the pool.")]
    Pool(#[from] deadpool_redis::PoolError),
    #[error("Valkey command failed.")]
    Redis(#[from] deadpool_redis::redis::RedisError),
}

#[tracing::instrument(name = "Check rate limit", skip(pool))]
pub async fn check_rate_limit(
    pool: &Pool,
    key: &str,
    max_requests: u64,
    window_seconds: u64,
) -> Result<RateLimitDecision, RateLimitError> {
    let mut connection = pool.get().await?;

    let count = connection.incr(key, 1).await?;
    if count == 1 {
        connection.expire(key, window_seconds as i64).await?;
    }

    Ok(if count as u64 > max_requests {
        RateLimitDecision::Limited
    } else {
        RateLimitDecision::Allowed
    })
}
