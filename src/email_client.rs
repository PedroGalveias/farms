use crate::domain::user::Email;
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};

pub struct EmailClient {
    http_client: Client,
    base_url: String,
    sender: Email,
    authorization_token: SecretString,
}

impl EmailClient {
    pub fn new(
        base_url: String,
        sender: Email,
        authorization_token: SecretString,
        timeout: std::time::Duration,
    ) -> Result<Self, anyhow::Error> {
        let http_client = Client::builder().timeout(timeout).build()?;

        Ok(Self {
            http_client,
            base_url,
            sender,
            authorization_token,
        })
    }

    #[tracing::instrument(name = "Send email", skip(self, html_content, text_content))]
    pub async fn send_email(
        &self,
        recipient: &Email,
        subject: &str,
        html_content: &str,
        text_content: &str,
    ) -> Result<(), reqwest::Error> {
        let url = format!("{}/email", self.base_url);

        let request_body = SendEmailRequest {
            from: self.sender.as_ref(),
            to: recipient.as_ref(),
            subject,
            html_body: html_content,
            text_body: text_content,
        };

        self.http_client
            .post(&url)
            .header(
                "X-Postmark-Server-Token",
                self.authorization_token.expose_secret(),
            )
            .json(&request_body)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "PascalCase")]
struct SendEmailRequest<'a> {
    from: &'a str,
    to: &'a str,
    subject: &'a str,
    html_body: &'a str,
    text_body: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use wiremock::matchers::{any, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_email() -> Email {
        Email::parse("test@farms.local".to_string()).unwrap()
    }

    fn email_client(base_url: String) -> EmailClient {
        EmailClient::new(
            base_url,
            test_email(),
            SecretString::from("test-token".to_string()),
            std::time::Duration::from_millis(200),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn send_email_sends_the_expected_request() {
        let server = MockServer::start().await;
        let client = email_client(server.uri());

        Mock::given(method("POST"))
            .and(path("/email"))
            .and(header("X-Postmark-Server-Token", "test-token"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let outcome = client
            .send_email(&test_email(), "subject", "<p>html</p>", "text")
            .await;

        assert_ok!(outcome);
    }

    #[tokio::test]
    async fn send_email_fails_on_server_error() {
        let server = MockServer::start().await;
        let client = email_client(server.uri());

        Mock::given(any())
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let outcome = client.send_email(&test_email(), "s", "h", "t").await;

        assert_err!(outcome);
    }

    #[tokio::test]
    async fn send_email_times_out_on_a_slow_response() {
        let server = MockServer::start().await;
        let client = email_client(server.uri());

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(60)))
            .mount(&server)
            .await;

        let outcome = client.send_email(&test_email(), "s", "h", "t").await;

        assert_err!(outcome);
    }
}
