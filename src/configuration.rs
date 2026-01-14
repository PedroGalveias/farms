use secrecy::{ExposeSecret, SecretString};
use serde::Deserializer;
use serde_aux::field_attributes::deserialize_number_from_string;
use sqlx::postgres::{PgConnectOptions, PgSslMode};

#[derive(serde::Deserialize, Clone)]
pub struct Settings {
    pub database: DatabaseSettings,
    pub application: ApplicationSettings,
    pub redis: RedisSettings,
    pub idempotency: IdempotencySettings,
}

#[derive(serde::Deserialize, Clone)]
pub struct ApplicationSettings {
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub base_url: String,
}

#[derive(serde::Deserialize, Clone)]
pub struct DatabaseSettings {
    pub username: String,
    pub password: SecretString,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub database_name: String,
    pub require_ssl: bool,
    pub max_connections: Option<u32>,
    pub timeout_seconds: Option<u64>,
}

#[derive(serde::Deserialize, Clone)]
pub struct RedisSettings {
    pub uri: SecretString,
    pub pool_max_size: Option<usize>,
    pub timeout_seconds: Option<u64>,
    pub session_key_prefix: String,
}

#[derive(serde::Deserialize, Clone)]
pub struct IdempotencySettings {
    pub engine: IdempotencyEngine,
    #[serde(default = "default_idempotency_settings_ttl_seconds")]
    pub ttl_seconds: u64,
    #[serde(default = "default_idempotency_settings_redis_key_prefix")]
    pub redis_key_prefix: String,
}

fn default_idempotency_settings_ttl_seconds() -> u64 {
    600 // 10 min
}

fn default_idempotency_settings_redis_key_prefix() -> String {
    "idem".to_string()
}

/// The runtime environment for our application.
pub enum Environment {
    Local,
    Production,
}
impl Environment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Production => "production",
        }
    }
}
impl TryFrom<String> for Environment {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "production" => Ok(Self::Production),
            other => Err(format!(
                "{} is not supported environment.\
                Use either `local` or `production`.",
                other
            )),
        }
    }
}

#[derive(Clone)]
pub enum IdempotencyEngine {
    None,
    Redis,
    Postgres,
}
impl IdempotencyEngine {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Redis => "redis",
            Self::Postgres => "postgres",
        }
    }
}
impl TryFrom<String> for IdempotencyEngine {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "none" => Ok(Self::None),
            "redis" => Ok(Self::Redis),
            "postgres" => Ok(Self::Postgres),
            other => Err(format!(
                "'{}' is not a supported Idempotency engine.\
                Use 'redis', 'postgres' or 'none' to disable Idempotency\
                Warning: postgres engine is currently untested",
                other
            )),
        }
    }
}
impl<'de> serde::Deserialize<'de> for IdempotencyEngine {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        IdempotencyEngine::try_from(s).map_err(serde::de::Error::custom)
    }
}

impl DatabaseSettings {
    pub fn without_db(&self) -> PgConnectOptions {
        let ssl_mode = if self.require_ssl {
            PgSslMode::Require
        } else {
            PgSslMode::Prefer
        };

        PgConnectOptions::new()
            .host(&self.host)
            .username(&self.username)
            .password(self.password.expose_secret())
            .port(self.port)
            .ssl_mode(ssl_mode)
    }

    pub fn with_db(&self) -> PgConnectOptions {
        self.without_db().database(&self.database_name)
    }
}

pub fn get_configuration() -> Result<Settings, config::ConfigError> {
    let base_path = std::env::current_dir().expect("Failed to determine the current directory");
    let configuration_directory = base_path.join("configuration");

    // Detect the running environment.
    // Default to `local` if unspecified.
    let environment: Environment = std::env::var("APP_ENVIRONMENT")
        .unwrap_or_else(|_| "local".into())
        .try_into()
        .expect("Failed to parse APP_ENVIRONMENT.");

    let environment_filename = format!("{}.yaml", environment.as_str());

    // Initialise our configuration reader
    let settings = config::Config::builder()
        .add_source(config::File::from(
            configuration_directory.join("base.yaml"),
        ))
        .add_source(config::File::from(
            configuration_directory.join(environment_filename),
        ))
        .add_source(
            config::Environment::with_prefix("APP")
                .prefix_separator("_")
                .separator("__"),
        )
        .build()?;

    // Try to convert the configuration values it read into
    // our Settings type
    settings.try_deserialize::<Settings>()
}
