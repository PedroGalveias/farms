use crate::idempotency::{
    HeaderPair, IdempotencyData, IdempotencyKey, persistence::IdempotencyPersistenceError,
};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::num::TryFromIntError;
use uuid::Uuid;

// TODO Remove dead code after further development and testing

#[allow(dead_code)]
pub enum PostgresPersistenceNextAction {
    StartProcessing(Transaction<'static, Postgres>),
    ReturnSavedData(IdempotencyData),
}

#[allow(dead_code)]
pub async fn try_processing(
    mut transaction: Transaction<'static, Postgres>,
    db_pool: &PgPool,
    idempotency_key: &IdempotencyKey,
    user_id: Uuid,
    ttl_seconds: u64,
) -> Result<PostgresPersistenceNextAction, IdempotencyPersistenceError> {
    let ttl_seconds = ttl_seconds_to_i64(ttl_seconds)?;
    let query = sqlx::query(
        r#"
        INSERT INTO idempotency (
            user_id,
            key,
            created_at,
            expire_at
        )
        VALUES ($1, $2, now(), now() + ($3::bigint * interval '1 second'))
        ON CONFLICT (user_id, key) DO UPDATE
        SET
            created_at = EXCLUDED.created_at,
            expire_at = EXCLUDED.expire_at,
            response_status_code = NULL,
            response_headers = NULL,
            response_body = NULL
        WHERE
            idempotency.expire_at <= now()
        "#,
    );
    let n_inserted_rows = query
        .bind(user_id)
        .bind(idempotency_key.as_ref())
        .bind(ttl_seconds)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
    if n_inserted_rows > 0 {
        Ok(PostgresPersistenceNextAction::StartProcessing(transaction))
    } else {
        let saved_response_data = get_saved_response(db_pool, idempotency_key, user_id)
            .await?
            .ok_or(IdempotencyPersistenceError::ExpectedResponseNotFoundError)?;

        Ok(PostgresPersistenceNextAction::ReturnSavedData(
            saved_response_data,
        ))
    }
}

#[allow(dead_code)]
pub async fn get_saved_response(
    pool: &PgPool,
    idempotency_key: &IdempotencyKey,
    user_id: Uuid,
) -> Result<Option<IdempotencyData>, IdempotencyPersistenceError> {
    let saved_response = sqlx::query(
        r#"
        SELECT
            response_status_code,
            response_headers,
            response_body
        FROM idempotency
        WHERE
            user_id = $1 AND
            key = $2 AND
            expire_at > now()
        "#,
    )
    .bind(user_id)
    .bind(idempotency_key.as_ref())
    .fetch_optional(pool)
    .await?;

    if let Some(r) = saved_response {
        let Some(response_status_code) = r.get::<Option<i16>, _>("response_status_code") else {
            return Ok(None);
        };
        let response_status_code: u16 = response_status_code
            .try_into()
            .map_err(|e: TryFromIntError| IdempotencyPersistenceError::UnexpectedError(e.into()))?;
        if response_status_code == 0 {
            return Ok(None);
        }

        let saved_response_data = IdempotencyData {
            response_status_code,
            response_headers: r
                .get::<Option<Vec<HeaderPair>>, _>("response_headers")
                .unwrap_or_default(),
            response_body: r
                .get::<Option<Vec<u8>>, _>("response_body")
                .unwrap_or_default(),
        };

        Ok(Some(saved_response_data))
    } else {
        Ok(None)
    }
}

#[allow(dead_code)]
pub async fn save_response(
    mut transaction: Transaction<'static, Postgres>,
    idempotency_key: &IdempotencyKey,
    user_id: Uuid,
    ttl_seconds: u64,
    idempotency_data: &IdempotencyData,
) -> Result<Transaction<'static, Postgres>, IdempotencyPersistenceError> {
    let ttl_seconds = ttl_seconds_to_i64(ttl_seconds)?;
    sqlx::query(
        r#"
        UPDATE idempotency
        SET
            response_status_code = $3,
            response_headers = $4,
            response_body = $5,
            expire_at = now() + ($6::bigint * interval '1 second')
        WHERE
            user_id = $1 AND
            key = $2
        "#,
    )
    .bind(user_id)
    .bind(idempotency_key.as_ref())
    .bind(idempotency_data.response_status_code as i16)
    .bind(&idempotency_data.response_headers)
    .bind(&idempotency_data.response_body)
    .bind(ttl_seconds)
    .execute(&mut *transaction)
    .await?;

    Ok(transaction)
}

fn ttl_seconds_to_i64(ttl_seconds: u64) -> Result<i64, IdempotencyPersistenceError> {
    i64::try_from(ttl_seconds)
        .map_err(|e: TryFromIntError| IdempotencyPersistenceError::UnexpectedError(e.into()))
}
