use deadpool_redis::{Pool, redis::Script};
use std::sync::LazyLock;

/// Increment the fixed-window counter and make sure it expires, atomically.
///
/// Running INCR and EXPIRE as two separate commands could leave a counter with
/// no expiry if the second one never ran (connection dropped, command timed
/// out, process died), and that client would stay rate-limited forever.
/// Valkey runs a script as a single step. Checking `TTL == -1` rather than
/// `count == 1` also gives an expiry to counters already left without one.
static RATE_LIMIT_SCRIPT: LazyLock<Script> = LazyLock::new(|| {
    Script::new(
        r"
        local count = redis.call('INCR', KEYS[1])
        if redis.call('TTL', KEYS[1]) == -1 then
            redis.call('EXPIRE', KEYS[1], ARGV[1])
        end
        return count
        ",
    )
});

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

    let count: u64 = RATE_LIMIT_SCRIPT
        .key(key)
        .arg(window_seconds)
        .invoke_async(&mut connection)
        .await?;

    Ok(if count > max_requests {
        RateLimitDecision::Limited
    } else {
        RateLimitDecision::Allowed
    })
}
