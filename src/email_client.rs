use crate::configuration::{EmailClientEngine, EmailClientSettings};
use crate::domain::user::Email;
use anyhow::Context;
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};

/// Email transport selected at startup from configuration.
///
/// `Mailjet` performs real HTTP delivery against the Mailjet Send API. `Log`
/// writes the message to the tracing log instead of sending it - this is what
/// local development uses, so the verification link can be copied straight from
/// the logs without provisioning a real email provider.
pub enum EmailClient {
    Mailjet(MailjetClient),
    Log(LogEmailClient),
}

impl EmailClient {
    /// Build the email backend chosen in configuration.
    pub fn from_settings(settings: &EmailClientSettings) -> Result<Self, anyhow::Error> {
        match settings.engine {
            EmailClientEngine::Mailjet => Ok(Self::Mailjet(MailjetClient::new(
                settings.base_url.clone(),
                settings.sender().context("Invalid sender email address.")?,
                settings.api_key.clone(),
                settings.secret_key.clone(),
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
            Self::Mailjet(client) => client
                .send_email(recipient, subject, html_content, text_content)
                .await
                .context("Failed to send email via Mailjet."),
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

/// Real email delivery via the Mailjet Send API (v3.1).
pub struct MailjetClient {
    http_client: Client,
    base_url: String,
    sender: Email,
    api_key: SecretString,
    secret_key: SecretString,
}

impl MailjetClient {
    pub fn new(
        base_url: String,
        sender: Email,
        api_key: SecretString,
        secret_key: SecretString,
        timeout: std::time::Duration,
    ) -> Result<Self, anyhow::Error> {
        let http_client = Client::builder().timeout(timeout).build()?;

        Ok(Self {
            http_client,
            base_url,
            sender,
            api_key,
            secret_key,
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
        // Mailjet's v3.1 transactional send endpoint.
        let url = format!("{}/v3.1/send", self.base_url);

        let request_body = SendEmailRequest {
            messages: vec![Message {
                from: Contact {
                    email: self.sender.as_ref(),
                },
                to: vec![Contact {
                    email: recipient.as_ref(),
                }],
                subject,
                text_part: text_content,
                html_part: html_content,
            }],
        };

        self.http_client
            .post(&url)
            // Mailjet authenticates with the API key (public) as the username
            // and the secret key (private) as the password, via HTTP Basic auth.
            .basic_auth(
                self.api_key.expose_secret(),
                Some(self.secret_key.expose_secret()),
            )
            .json(&request_body)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}

/// Request payload for Mailjet's `POST /v3.1/send` endpoint.
#[derive(serde::Serialize)]
struct SendEmailRequest<'a> {
    #[serde(rename = "Messages")]
    messages: Vec<Message<'a>>,
}

#[derive(serde::Serialize)]
struct Message<'a> {
    #[serde(rename = "From")]
    from: Contact<'a>,
    #[serde(rename = "To")]
    to: Vec<Contact<'a>>,
    #[serde(rename = "Subject")]
    subject: &'a str,
    #[serde(rename = "TextPart")]
    text_part: &'a str,
    #[serde(rename = "HTMLPart")]
    html_part: &'a str,
}

#[derive(serde::Serialize)]
struct Contact<'a> {
    #[serde(rename = "Email")]
    email: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use wiremock::matchers::{any, header_regex, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_email() -> Email {
        Email::parse("test@farms.local".to_string()).unwrap()
    }

    fn email_client(base_url: String) -> MailjetClient {
        MailjetClient::new(
            base_url,
            test_email(),
            SecretString::from("test-key".to_string()),
            SecretString::from("test-secret".to_string()),
            std::time::Duration::from_millis(200),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn send_email_sends_the_expected_request() {
        let server = MockServer::start().await;
        let client = email_client(server.uri());

        Mock::given(method("POST"))
            .and(path("/v3.1/send"))
            // HTTP Basic auth header is present (base64 of "test-key:test-secret").
            .and(header_regex("Authorization", "^Basic "))
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
