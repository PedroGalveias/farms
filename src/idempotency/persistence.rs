use crate::{
    configuration::IdempotencySettings,
    idempotency::{IdempotencyError, IdempotencyKey},
};
use actix_web::{body::to_bytes, http::StatusCode, HttpResponse};
use deadpool_redis::{
    redis::{AsyncCommands, AsyncTypedCommands, ExistenceCheck, SetExpiry, SetOptions},
    Pool,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq)]
pub struct HeaderPair {
    pub name: String,
    pub value: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct RedisIdempotency {
    pub response_status_code: Option<u16>,
    pub response_headers: Vec<HeaderPair>,
    pub response_body: Vec<u8>,
}

async fn redis_get_saved_response(
    pool: &Pool,
    idempotency_key: &IdempotencyKey,
) -> Result<Option<HttpResponse>, IdempotencyError> {
    let mut connection = pool.get().await?;
    let bytes: Option<Vec<u8>> =
        AsyncCommands::get(&mut connection, idempotency_key.as_ref()).await?;

    let Some(bytes) = bytes else {
        return Ok(None);
    };

    let data: RedisIdempotency = rmp_serde::from_slice(&bytes)?;

    let Some(status_code) = data.response_status_code else {
        return Ok(None);
    };

    let status_code = StatusCode::from_u16(status_code)
        .map_err(|e| IdempotencyError::UnexpectedError(e.into()))?;
    let mut response = HttpResponse::build(status_code);

    for HeaderPair { name, value } in data.response_headers {
        response.append_header((name, value));
    }

    Ok(Some(response.body(data.response_body)))
}

pub async fn save_response(
    pool: &Pool,
    idempotency_key: &str,
    idempotency_settings: &IdempotencySettings,
    http_response: HttpResponse,
) -> Result<HttpResponse, IdempotencyError> {
    let redis_settings = idempotency_settings
        .redis
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing Redis Idempotency Settings."))?;

    // Add user_id to key?
    let idempotency_key =
        IdempotencyKey::try_from(format!("{}:{}", redis_settings.key_prefix, idempotency_key))
            .map_err(|e| IdempotencyError::UnexpectedError(e.into()))?;

    redis_save_response(
        pool,
        &idempotency_key,
        redis_settings.ttl_seconds,
        http_response,
    )
    .await
}

async fn redis_save_response(
    pool: &Pool,
    idempotency_key: &IdempotencyKey,
    ttl_seconds: u64,
    http_response: HttpResponse,
) -> Result<HttpResponse, IdempotencyError> {
    let (response_head, body) = http_response.into_parts();

    let body = to_bytes(body).await.map_err(|e| anyhow::anyhow!("{}", e))?;
    let status_code = response_head.status().as_u16();
    let headers = {
        let mut h = Vec::with_capacity(response_head.headers().len());
        for (name, value) in response_head.headers().iter() {
            let name = name.as_str().to_owned();
            let value = value.as_bytes().to_owned();
            h.push(HeaderPair { name, value });
        }
        h
    };
    let data = RedisIdempotency {
        response_status_code: Some(status_code),
        response_headers: headers,
        response_body: body.to_vec(),
    };
    let data = rmp_serde::to_vec(&data)?;

    redis_set_ex(pool, idempotency_key.as_ref(), data, ttl_seconds).await?;

    let http_response = response_head.set_body(body).map_into_boxed_body();
    Ok(http_response)
}

async fn redis_set_ex(
    pool: &Pool,
    key: &str,
    value: Vec<u8>,
    ttl_seconds: u64,
) -> Result<(), anyhow::Error> {
    let mut connection = pool.get().await?;
    AsyncTypedCommands::set_ex(&mut connection, key, value, ttl_seconds).await?;
    Ok(())
}

pub enum IdempotencyNextAction {
    StartProcessing,
    ReturnSavedResponse(HttpResponse),
}

pub async fn try_processing(
    pool: &Pool,
    idempotency_key: &str,
    idempotency_settings: &IdempotencySettings,
) -> Result<IdempotencyNextAction, IdempotencyError> {
    let redis_settings = idempotency_settings
        .redis
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing Redis Idempotency Settings."))?;

    // Add user_id to key?
    let idempotency_key =
        IdempotencyKey::try_from(format!("{}:{}", redis_settings.key_prefix, idempotency_key))
            .map_err(|e| IdempotencyError::UnexpectedError(e.into()))?;

    let data = RedisIdempotency {
        response_status_code: None,
        response_headers: Vec::new(),
        response_body: Vec::new(),
    };
    let data = rmp_serde::to_vec(&data)?;

    let mut connection = pool.get().await?;

    let result: Option<String> = AsyncTypedCommands::set_options(
        &mut connection,
        idempotency_key.as_ref(),
        &data,
        SetOptions::default()
            .conditional_set(ExistenceCheck::NX)
            .with_expiration(SetExpiry::EX(redis_settings.ttl_seconds)),
    )
    .await?;

    if result.is_some() {
        Ok(IdempotencyNextAction::StartProcessing)
    } else {
        let saved_response = redis_get_saved_response(pool, &idempotency_key)
            .await?
            .ok_or(IdempotencyError::ExpectedResponseNotFoundError)?;

        Ok(IdempotencyNextAction::ReturnSavedResponse(saved_response))
    }
}
