use std::collections::HashMap;
use std::str::FromStr;

pub type EnvVars = HashMap<String, String>;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("variável de ambiente obrigatória ausente: {0}")]
    Missing(&'static str),

    #[error("valor inválido para {key}: {value:?} ({reason})")]
    Invalid {
        key: &'static str,
        value: String,
        reason: String,
    },
}

pub fn env_vars() -> EnvVars {
    let _ = dotenvy::dotenv();
    std::env::vars().collect()
}

fn get<'a>(vars: &'a EnvVars, key: &str) -> Option<&'a str> {
    vars.get(key).map(String::as_str).filter(|v| !v.is_empty())
}

fn required(vars: &EnvVars, key: &'static str) -> Result<String, ConfigError> {
    get(vars, key)
        .map(str::to_owned)
        .ok_or(ConfigError::Missing(key))
}

fn parse_or<T>(vars: &EnvVars, key: &'static str, default: T) -> Result<T, ConfigError>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    match get(vars, key) {
        None => Ok(default),
        Some(raw) => raw.parse().map_err(|e: T::Err| ConfigError::Invalid {
            key,
            value: raw.to_owned(),
            reason: e.to_string(),
        }),
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub service_name: String,
    pub host: String,
    pub port: u16,
    pub log: LogConfig,
}

impl AppConfig {
    pub fn from_env(default_service_name: &str) -> Result<Self, ConfigError> {
        Self::from_source(&env_vars(), default_service_name)
    }

    pub fn from_source(vars: &EnvVars, default_service_name: &str) -> Result<Self, ConfigError> {
        Ok(Self {
            service_name: get(vars, "SERVICE_NAME")
                .unwrap_or(default_service_name)
                .to_owned(),
            host: get(vars, "HTTP_HOST").unwrap_or("0.0.0.0").to_owned(),
            port: parse_or(vars, "HTTP_PORT", 8080)?,
            log: LogConfig::from_source(vars)?,
        })
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone)]
pub struct LogConfig {
    pub level: String,
    pub format: LogFormat,
}

impl LogConfig {
    pub fn from_source(vars: &EnvVars) -> Result<Self, ConfigError> {
        Ok(Self {
            level: get(vars, "LOG_LEVEL")
                .or_else(|| get(vars, "RUST_LOG"))
                .unwrap_or("info")
                .to_owned(),
            format: parse_or(vars, "LOG_FORMAT", LogFormat::Pretty)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Pretty,
    Json,
}

impl FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "pretty" | "text" => Ok(Self::Pretty),
            "json" => Ok(Self::Json),
            other => Err(format!("esperado 'pretty' ou 'json', recebido '{other}'")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub connect_timeout_secs: u64,
}

impl DatabaseConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_source(&env_vars())
    }

    pub fn from_source(vars: &EnvVars) -> Result<Self, ConfigError> {
        Ok(Self {
            url: required(vars, "DATABASE_URL")?,
            max_connections: parse_or(vars, "DATABASE_MAX_CONNECTIONS", 10)?,
            connect_timeout_secs: parse_or(vars, "DATABASE_CONNECT_TIMEOUT_SECS", 5)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AmqpConfig {
    pub url: String,
    pub exchange: String,
    pub prefetch: u16,
}

impl AmqpConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_source(&env_vars())
    }

    pub fn from_source(vars: &EnvVars) -> Result<Self, ConfigError> {
        Ok(Self {
            url: required(vars, "AMQP_URL")?,
            exchange: get(vars, "AMQP_EXCHANGE")
                .unwrap_or("marketplace")
                .to_owned(),
            prefetch: parse_or(vars, "AMQP_PREFETCH", 16)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> EnvVars {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn app_config_usa_defaults_quando_ambiente_vazio() {
        let cfg = AppConfig::from_source(&vars(&[]), "catalog").unwrap();

        assert_eq!(cfg.service_name, "catalog");
        assert_eq!(cfg.bind_addr(), "0.0.0.0:8080");
        assert_eq!(cfg.log.level, "info");
        assert_eq!(cfg.log.format, LogFormat::Pretty);
    }

    #[test]
    fn app_config_respeita_overrides() {
        let cfg = AppConfig::from_source(
            &vars(&[
                ("SERVICE_NAME", "orders"),
                ("HTTP_HOST", "127.0.0.1"),
                ("HTTP_PORT", "8082"),
                ("LOG_FORMAT", "json"),
                ("LOG_LEVEL", "debug"),
            ]),
            "catalog",
        )
        .unwrap();

        assert_eq!(cfg.service_name, "orders");
        assert_eq!(cfg.bind_addr(), "127.0.0.1:8082");
        assert_eq!(cfg.log.level, "debug");
        assert_eq!(cfg.log.format, LogFormat::Json);
    }

    #[test]
    fn variavel_vazia_equivale_a_ausente() {
        let cfg = AppConfig::from_source(&vars(&[("SERVICE_NAME", "")]), "catalog").unwrap();

        assert_eq!(cfg.service_name, "catalog");
    }

    #[test]
    fn log_level_cai_para_rust_log() {
        let cfg = LogConfig::from_source(&vars(&[("RUST_LOG", "debug,sqlx=warn")])).unwrap();

        assert_eq!(cfg.level, "debug,sqlx=warn");
    }

    #[test]
    fn porta_invalida_retorna_erro_com_contexto() {
        let err =
            AppConfig::from_source(&vars(&[("HTTP_PORT", "oito mil")]), "catalog").unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("HTTP_PORT"), "mensagem inesperada: {msg}");
        assert!(msg.contains("oito mil"), "mensagem inesperada: {msg}");
    }

    #[test]
    fn formato_de_log_invalido_retorna_erro() {
        let err = LogConfig::from_source(&vars(&[("LOG_FORMAT", "xml")])).unwrap_err();

        assert!(err.to_string().contains("LOG_FORMAT"));
    }

    #[test]
    fn database_url_e_obrigatoria() {
        let err = DatabaseConfig::from_source(&vars(&[])).unwrap_err();

        assert!(matches!(err, ConfigError::Missing("DATABASE_URL")));
    }

    #[test]
    fn database_config_le_url_e_defaults() {
        let cfg = DatabaseConfig::from_source(&vars(&[(
            "DATABASE_URL",
            "postgres://user:pass@localhost/catalog",
        )]))
        .unwrap();

        assert_eq!(cfg.url, "postgres://user:pass@localhost/catalog");
        assert_eq!(cfg.max_connections, 10);
        assert_eq!(cfg.connect_timeout_secs, 5);
    }

    #[test]
    fn amqp_config_le_url_e_defaults() {
        let cfg = AmqpConfig::from_source(&vars(&[(
            "AMQP_URL",
            "amqp://guest:guest@localhost:5672/%2f",
        )]))
        .unwrap();

        assert_eq!(cfg.exchange, "marketplace");
        assert_eq!(cfg.prefetch, 16);
    }

    #[test]
    fn amqp_url_e_obrigatoria() {
        let err = AmqpConfig::from_source(&vars(&[])).unwrap_err();

        assert!(matches!(err, ConfigError::Missing("AMQP_URL")));
    }
}
