use crate::configuration::{EmailClientEngine, EmailClientSettings};
use crate::domain::user::Email;
use anyhow::Context;
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};

/// Email transport selected at startup from configuration.
///
/// `ZeptoMail` performs real HTTP delivery against the ZeptoMail API. `Log`
/// writes the message to the tracing log instead of sending it - this is what
/// local development uses, so the verification link can be copied straight from
/// the logs without provisioning a real email provider.
pub enum EmailClient {
    ZeptoMail(ZeptoMailClient),
    Log(LogEmailClient),
}

impl EmailClient {
    /// Build the email backend chosen in configuration.
    pub fn from_settings(settings: &EmailClientSettings) -> Result<Self, anyhow::Error> {
        match settings.engine {
            EmailClientEngine::ZeptoMail => Ok(Self::ZeptoMail(ZeptoMailClient::new(
                settings.base_url.clone(),
                settings.sender().context("Invalid sender email address.")?,
                settings.authorization_token.clone(),
                settings.timeout(),
            )?)),
            EmailClientEngine::Log => Ok(Self::Log(LogEmailClient)),
        }
    }

    /// Dispatch to the active backend.
    pub async fn send_email(
        &self,
        recipient: &Email,
        subject: &str,
        html_content: &str,
        text_content: &str,
    ) -> Result<(), anyhow::Error> {
        match self {
            Self::ZeptoMail(client) => client
                .send_email(recipient, subject, html_content, text_content)
                .await
                .context("Failed to send email via ZeptoMail."),
            Self::Log(client) => {
                client
                    .send_email(recipient, subject, html_content, text_content)
                    .await
            }
        }
    }
}

/// Logs emails instead of sending them. Used in local development so the
/// verification link is visible in the application logs.
pub struct LogEmailClient;

impl LogEmailClient {
    #[tracing::instrument(
        name = "Log email (no delivery)",
        skip(self, _html_content, text_content)
    )]
    pub async fn send_email(
        &self,
        recipient: &Email,
        subject: &str,
        _html_content: &str,
        text_content: &str,
    ) -> Result<(), anyhow::Error> {
        tracing::info!(
            recipient = recipient.as_ref(),
            subject = subject,
            "Email backend is 'log': not sending. Message body follows:\n{text_content}"
        );
        Ok(())
    }
}

/// Real email delivery via the ZeptoMail transactional API.
pub struct ZeptoMailClient {
    http_client: Client,
    base_url: String,
    sender: Email,
    authorization_token: SecretString,
}

impl ZeptoMailClient {
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
        // ZeptoMail's transactional send endpoint.
        let url = format!("{}/v1.1/email", self.base_url);

        let request_body = SendEmailRequest {
            from: EmailAddress {
                address: self.sender.as_ref(),
            },
            to: vec![Recipient {
                email_address: EmailAddress {
                    address: recipient.as_ref(),
                },
            }],
            subject,
            htmlbody: html_content,
            textbody: text_content,
        };

        self.http_client
            .post(&url)
            // ZeptoMail authenticates with a "Send Mail" token, sent as
            // `Authorization: Zoho-enczapikey <token>`.
            .header(
                "Authorization",
                format!(
                    "Zoho-enczapikey {}",
                    self.authorization_token.expose_secret()
                ),
            )
            .json(&request_body)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}

/// Request payload for ZeptoMail's `POST /v1.1/email` endpoint.
#[derive(serde::Serialize)]
struct SendEmailRequest<'a> {
    from: EmailAddress<'a>,
    to: Vec<Recipient<'a>>,
    subject: &'a str,
    htmlbody: &'a str,
    textbody: &'a str,
}

#[derive(serde::Serialize)]
struct Recipient<'a> {
    email_address: EmailAddress<'a>,
}

#[derive(serde::Serialize)]
struct EmailAddress<'a> {
    address: &'a str,
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

    fn email_client(base_url: String) -> ZeptoMailClient {
        ZeptoMailClient::new(
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
            .and(path("/v1.1/email"))
            .and(header("Authorization", "Zoho-enczapikey test-token"))
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
