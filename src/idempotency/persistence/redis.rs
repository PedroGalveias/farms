use crate::{
    configuration::IdempotencySettings,
    idempotency::{IdempotencyData, IdempotencyKey, persistence::IdempotencyPersistenceError},
};
use deadpool_redis::{
    Pool,
    redis::{AsyncCommands, AsyncTypedCommands, ExistenceCheck, SetExpiry, SetOptions},
};

pub enum RedisPersistenceNextAction {
    StartProcessing,
    ReturnSavedData(IdempotencyData),
}

pub async fn try_processing(
    pool: &Pool,
    idempotency_key: &IdempotencyKey,
    idempotency_settings: &IdempotencySettings,
) -> Result<RedisPersistenceNextAction, IdempotencyPersistenceError> {
    // let data = IdempotencyData {
    //     response_status_code: 0,
    //     response_headers: Vec::new(),
    //     response_body: Vec::new(),
    // };
    // let data = rmp_serde::to_vec(&data)?;
    let data: Vec<u8> = Vec::new();

    let mut connection = pool.get().await?;

    let result: Option<String> = AsyncTypedCommands::set_options(
        &mut connection,
        idempotency_key.as_ref(),
        &data,
        SetOptions::default()
            .conditional_set(ExistenceCheck::NX)
            .with_expiration(SetExpiry::EX(idempotency_settings.ttl_seconds)),
    )
    .await?;

    if result.is_some() {
        Ok(RedisPersistenceNextAction::StartProcessing)
    } else {
        let saved_response_data = get_saved_response(pool, &idempotency_key)
            .await?
            .ok_or(IdempotencyPersistenceError::ExpectedResponseNotFoundError)?;

        Ok(RedisPersistenceNextAction::ReturnSavedData(
            saved_response_data,
        ))
    }
}

pub async fn get_saved_response(
    pool: &Pool,
    idempotency_key: &IdempotencyKey,
) -> Result<Option<IdempotencyData>, IdempotencyPersistenceError> {
    let mut connection = pool.get().await?;
    let bytes: Option<Vec<u8>> =
        AsyncCommands::get(&mut connection, idempotency_key.as_ref()).await?;

    let Some(bytes) = bytes else {
        return Ok(None);
    };

    if bytes.is_empty() {
        return Ok(None);
    }

    let data: IdempotencyData = rmp_serde::from_slice(&bytes)?;

    Ok(Some(data))
}

pub async fn save_response(
    pool: &Pool,
    idempotency_key: &IdempotencyKey,
    ttl_seconds: u64,
    idempotency_data: &IdempotencyData,
) -> Result<(), IdempotencyPersistenceError> {
    let data_bytes = rmp_serde::to_vec(idempotency_data)?;

    let mut connection = pool.get().await?;
    AsyncTypedCommands::set_ex(
        &mut connection,
        idempotency_key.as_ref(),
        data_bytes,
        ttl_seconds,
    )
    .await?;

    Ok(())
}
