use crate::{
    configuration::{IdempotencyEngine, IdempotencySettings},
    idempotency::{
        IdempotencyData, IdempotencyError, IdempotencyKey,
        persistence::{
            //postgres::PostgresPersistenceNextAction,
            redis::RedisPersistenceNextAction,
        },
    },
};
use actix_web::HttpResponse;
use deadpool_redis::Pool;
use sqlx::{PgPool, Postgres, Transaction};

mod error;
mod postgres;
mod redis;

pub use error::IdempotencyPersistenceError;

pub async fn save_response(
    redis_pool: &Pool,
    transaction: Transaction<'static, Postgres>,
    idempotency_key: &str,
    //user_id: Uuid,
    idempotency_settings: &IdempotencySettings,
    http_response: HttpResponse,
) -> Result<(HttpResponse, Transaction<'static, Postgres>), IdempotencyError> {
    let idempotency_data = IdempotencyData::try_from_response(http_response).await?;
    match idempotency_settings.engine {
        // No idempotency just return the provided response
        IdempotencyEngine::None => Ok((idempotency_data.into_response()?, transaction)),
        IdempotencyEngine::Redis => {
            let idempotency_key = IdempotencyKey::try_from(format!(
                "{}:{}", // Add an extra ':{}' when user_id is available
                idempotency_settings.redis_key_prefix,
                //user_id.to_string(),
                idempotency_key
            ))
            .map_err(|e| IdempotencyError::UnexpectedError(e.into()))?;
            redis::save_response(
                redis_pool,
                &idempotency_key,
                idempotency_settings.ttl_seconds,
                &idempotency_data,
            )
            .await
            .map_err(IdempotencyError::from)?;

            Ok((idempotency_data.into_response()?, transaction))
        }
        // IdempotencyEngine::Postgres => {
        //     let idempotency_key = IdempotencyKey::try_from(idempotency_key.to_string())
        //         .map_err(|e| IdempotencyError::UnexpectedError(e.into()))?;
        //     let transaction =
        //         postgres::save_response(transaction, &idempotency_key, user_id, &idempotency_data)
        //             .await
        //             .map_err(IdempotencyError::from)?;
        //
        //     Ok((idempotency_data.into_response()?, transaction))
        // }
        // To enable postgres engine uncomment the match above and comment line bellow
        _ => Err(IdempotencyError::InvalidEngineError),
    }
}

pub enum IdempotencyNextAction {
    StartProcessing(Transaction<'static, Postgres>),
    ReturnSavedResponse(HttpResponse),
}

pub async fn try_processing(
    redis_pool: &Pool,
    db_pool: &PgPool,
    idempotency_key: &str,
    //user_id: Uuid,
    idempotency_settings: &IdempotencySettings,
) -> Result<IdempotencyNextAction, IdempotencyError> {
    let transaction = db_pool
        .begin()
        .await
        .map_err(IdempotencyPersistenceError::from)?;

    match idempotency_settings.engine {
        IdempotencyEngine::None => Ok(IdempotencyNextAction::StartProcessing(transaction)),
        IdempotencyEngine::Redis => {
            let idempotency_key = IdempotencyKey::try_from(format!(
                "{}:{}", // Add an extra ':{}' when user_id is available
                idempotency_settings.redis_key_prefix,
                //user_id.to_string(),
                idempotency_key
            ))
            .map_err(|e| IdempotencyError::UnexpectedError(e.into()))?;

            match redis::try_processing(redis_pool, &idempotency_key, idempotency_settings)
                .await
                .map_err(|e| match e {
                    IdempotencyPersistenceError::ExpectedResponseNotFoundError => {
                        IdempotencyError::ExpectedResponseNotFoundError
                    }
                    _ => IdempotencyError::from(e),
                })? {
                RedisPersistenceNextAction::ReturnSavedData(response_data) => Ok(
                    IdempotencyNextAction::ReturnSavedResponse(response_data.into_response()?),
                ),
                RedisPersistenceNextAction::StartProcessing => {
                    Ok(IdempotencyNextAction::StartProcessing(transaction))
                }
            }
        }
        // IdempotencyEngine::Postgres => {
        //     let idempotency_key = IdempotencyKey::try_from(idempotency_key.to_string())
        //         .map_err(|e| IdempotencyError::UnexpectedError(e.into()))?;
        //
        //     match postgres::try_processing(transaction, db_pool, &idempotency_key, user_id)
        //         .await
        //         .map_err(|e| match e {
        //             IdempotencyPersistenceError::ExpectedResponseNotFoundError => {
        //                 IdempotencyError::ExpectedResponseNotFoundError
        //             }
        //             _ => IdempotencyError::from(e),
        //         })? {
        //         PostgresPersistenceNextAction::ReturnSavedData(response_data) => Ok(
        //             IdempotencyNextAction::ReturnSavedResponse(response_data.into_response()?),
        //         ),
        //         PostgresPersistenceNextAction::StartProcessing(transaction) => {
        //             Ok(IdempotencyNextAction::StartProcessing(transaction))
        //         }
        //     }
        // }
        //To enable postgres engine uncomment the match above and comment line bellow
        _ => Err(IdempotencyError::InvalidEngineError),
    }
}
