use crate::idempotency::{
    HeaderPair, IdempotencyData, IdempotencyKey, persistence::IdempotencyPersistenceError,
};
use sqlx::{Executor, PgPool, Postgres, Transaction};
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
) -> Result<PostgresPersistenceNextAction, IdempotencyPersistenceError> {
    let query = sqlx::query!(
        r#"
        INSERT INTO idempotency (
            user_id,
            key,
            created_at
        )
        VALUES ($1, $2, now())
        ON CONFLICT DO NOTHING
        "#,
        user_id,
        idempotency_key.as_ref(),
    );
    let n_inserted_rows = transaction.execute(query).await?.rows_affected();
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
    let saved_response = sqlx::query!(
        r#"
        SELECT
            response_status_code as "response_status_code!",
            response_headers as "response_headers!: Vec<HeaderPair>",
            response_body as "response_body!"
        FROM idempotency
        WHERE
            user_id = $1 AND
            key = $2
        "#,
        user_id,
        idempotency_key.as_ref(),
    )
    .fetch_optional(pool)
    .await?;

    if let Some(r) = saved_response {
        let response_status_code: u16 = r
            .response_status_code
            .try_into()
            .map_err(|e: TryFromIntError| IdempotencyPersistenceError::UnexpectedError(e.into()))?;
        if response_status_code == 0 {
            return Ok(None);
        }

        let saved_response_data = IdempotencyData {
            response_status_code,
            response_headers: r.response_headers,
            response_body: r.response_body,
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
    idempotency_data: &IdempotencyData,
) -> Result<Transaction<'static, Postgres>, IdempotencyPersistenceError> {
    sqlx::query_unchecked!(
        r#"
        UPDATE idempotency
        SET
            response_status_code = $3,
            response_headers = $4,
            response_body = $5
        WHERE
            user_id = $1 AND
            key = $2
        "#,
        user_id,
        idempotency_key.as_ref(),
        idempotency_data.response_status_code as i16,
        idempotency_data.response_headers,
        idempotency_data.response_body,
    )
    .execute(&mut *transaction)
    .await?;

    Ok(transaction)
}
