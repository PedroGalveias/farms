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
