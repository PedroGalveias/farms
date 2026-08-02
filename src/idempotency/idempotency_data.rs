use actix_web::{HttpResponse, body::to_bytes, http::StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, sqlx::Type, Debug)]
#[sqlx(type_name = "header_pair")]
pub struct HeaderPair {
    pub name: String,
    pub value: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct IdempotencyData {
    pub response_status_code: u16,
    pub response_headers: Vec<HeaderPair>,
    pub response_body: Vec<u8>,
}
impl IdempotencyData {
    pub async fn try_from_response(http_response: HttpResponse) -> Result<Self, anyhow::Error> {
        let (response_head, body) = http_response.into_parts();

        let body_bytes = to_bytes(body).await.map_err(|e| anyhow::anyhow!("{}", e))?;
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

        Ok(Self {
            response_status_code: status_code,
            response_headers: headers,
            response_body: body_bytes.to_vec(),
        })
    }

    pub fn into_response(self) -> Result<HttpResponse, anyhow::Error> {
        if self.response_status_code == 0 {
            return Err(anyhow::anyhow!("No available StatusCode to build Response"));
        }
        let status_code = StatusCode::from_u16(self.response_status_code)?;
        let mut response = HttpResponse::build(status_code);

        for HeaderPair { name, value } in self.response_headers {
            response.append_header((name, value));
        }

        Ok(response.body(self.response_body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn try_from_response_extracts_status_headers_and_body() {
        let response = HttpResponse::Created()
            .insert_header(("X-Test-Header", "test-value"))
            .body("hello world");

        let data = IdempotencyData::try_from_response(response)
            .await
            .expect("Failed to build IdempotencyData from response.");

        assert_eq!(data.response_status_code, 201);
        assert_eq!(data.response_body, b"hello world".to_vec());
        assert!(
            data.response_headers
                .iter()
                .any(|h| h.name == "x-test-header" && h.value == b"test-value".to_vec())
        );
    }

    #[tokio::test]
    async fn try_from_response_handles_an_empty_body() {
        let response = HttpResponse::NoContent().finish();

        let data = IdempotencyData::try_from_response(response)
            .await
            .expect("Failed to build IdempotencyData from response.");

        assert_eq!(data.response_status_code, 204);
        assert!(data.response_body.is_empty());
    }

    #[test]
    fn into_response_rebuilds_status_headers_and_body() {
        let data = IdempotencyData {
            response_status_code: 201,
            response_headers: vec![HeaderPair {
                name: "x-test-header".to_string(),
                value: b"test-value".to_vec(),
            }],
            response_body: b"hello world".to_vec(),
        };

        let response = data
            .into_response()
            .expect("Failed to build response from IdempotencyData.");

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response.headers().get("x-test-header").unwrap(),
            "test-value"
        );
    }

    #[test]
    fn into_response_fails_when_status_code_is_zero() {
        let data = IdempotencyData {
            response_status_code: 0,
            response_headers: Vec::new(),
            response_body: Vec::new(),
        };

        let result = data.into_response();

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn try_from_response_and_into_response_round_trip() {
        let original = HttpResponse::Created()
            .insert_header(("X-Test-Header", "test-value"))
            .body("hello world");

        let data = IdempotencyData::try_from_response(original)
            .await
            .expect("Failed to build IdempotencyData from response.");
        let rebuilt = data
            .into_response()
            .expect("Failed to build response from IdempotencyData.");

        assert_eq!(rebuilt.status(), StatusCode::CREATED);
        assert_eq!(
            rebuilt.headers().get("x-test-header").unwrap(),
            "test-value"
        );

        let body = to_bytes(rebuilt.into_body())
            .await
            .expect("Failed to read response body.");
        assert_eq!(body.as_ref(), b"hello world");
    }
}
