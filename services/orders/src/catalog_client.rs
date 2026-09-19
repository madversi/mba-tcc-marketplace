use std::sync::Arc;
use std::time::Duration;

use moka::sync::Cache;
use reqwest::StatusCode;
use resilience::{CircuitBreaker, CircuitBreakerError, CircuitState, RetryPolicy};
use serde::Deserialize;
use shared::config::{env_vars, parse_or};
use shared::{ConfigError, EnvVars};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogProduct {
    pub id: Uuid,
    pub price: i64,
    pub active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catálogo indisponível: {0}")]
    Unavailable(#[from] reqwest::Error),
    #[error("catálogo indisponível: circuito aberto")]
    CircuitOpen,
    #[error("resposta inesperada do catálogo: {0}")]
    Unexpected(StatusCode),
}

#[derive(Debug, Clone, Copy)]
pub struct CatalogResilience {
    pub timeout_enabled: bool,
    pub timeout: Duration,
    pub retry_enabled: bool,
    pub retry: RetryPolicy,
    pub breaker_enabled: bool,
    pub breaker_failure_threshold: u32,
    pub breaker_open_timeout: Duration,
    pub fallback_enabled: bool,
    pub cache_ttl: Duration,
    pub cache_max_entries: u64,
}

impl CatalogResilience {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_source(&env_vars())
    }

    pub fn from_source(vars: &EnvVars) -> Result<Self, ConfigError> {
        let ms = |key: &'static str, default: u64| {
            parse_or(vars, key, default).map(Duration::from_millis)
        };

        Ok(Self {
            timeout_enabled: parse_or(vars, "CATALOG_TIMEOUT_ENABLED", true)?,
            timeout: ms("CATALOG_TIMEOUT_MS", 1_000)?,
            retry_enabled: parse_or(vars, "CATALOG_RETRY_ENABLED", true)?,
            retry: RetryPolicy::new(
                parse_or(vars, "CATALOG_RETRY_ATTEMPTS", 3)?,
                ms("CATALOG_RETRY_BASE_DELAY_MS", 100)?,
                ms("CATALOG_RETRY_MAX_DELAY_MS", 1_000)?,
            ),
            breaker_enabled: parse_or(vars, "CATALOG_BREAKER_ENABLED", true)?,
            breaker_failure_threshold: parse_or(vars, "CATALOG_BREAKER_FAILURE_THRESHOLD", 5)?,
            breaker_open_timeout: ms("CATALOG_BREAKER_OPEN_TIMEOUT_MS", 10_000)?,
            fallback_enabled: parse_or(vars, "CATALOG_FALLBACK_ENABLED", true)?,
            cache_ttl: Duration::from_secs(parse_or(vars, "CATALOG_CACHE_TTL_SECS", 600)?),
            cache_max_entries: parse_or(vars, "CATALOG_CACHE_MAX_ENTRIES", 10_000)?,
        })
    }

    pub fn all_disabled(self) -> Self {
        Self {
            timeout_enabled: false,
            retry_enabled: false,
            breaker_enabled: false,
            fallback_enabled: false,
            ..self
        }
    }
}

enum Fetched {
    Found(CatalogProduct),
    NotFound,
    Rejected(StatusCode),
}

#[derive(Clone)]
pub struct CatalogClient {
    base_url: String,
    http: reqwest::Client,
    retry: RetryPolicy,
    breaker: Option<Arc<CircuitBreaker>>,
    fallback: Option<Cache<Uuid, CatalogProduct>>,
}

impl CatalogClient {
    pub fn new(base_url: impl Into<String>, resilience: CatalogResilience) -> Self {
        resilience::report_mechanism("catalog_timeout", resilience.timeout_enabled);
        resilience::report_mechanism("catalog_retry", resilience.retry_enabled);
        resilience::report_mechanism("catalog_circuit_breaker", resilience.breaker_enabled);
        resilience::report_mechanism("catalog_fallback", resilience.fallback_enabled);
        tracing::info!(?resilience, "resiliência do cliente do catálogo");

        let mut http = reqwest::Client::builder();
        if resilience.timeout_enabled {
            http = http.timeout(resilience.timeout);
        }

        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            http: http
                .build()
                .expect("configuração válida do cliente HTTP do catálogo"),
            retry: if resilience.retry_enabled {
                resilience.retry
            } else {
                RetryPolicy::new(1, Duration::ZERO, Duration::ZERO)
            },
            breaker: resilience.breaker_enabled.then(|| {
                Arc::new(CircuitBreaker::new(
                    "catalog",
                    resilience.breaker_failure_threshold,
                    resilience.breaker_open_timeout,
                ))
            }),
            fallback: resilience.fallback_enabled.then(|| {
                Cache::builder()
                    .max_capacity(resilience.cache_max_entries)
                    .time_to_live(resilience.cache_ttl)
                    .build()
            }),
        }
    }

    pub fn circuit_state(&self) -> Option<CircuitState> {
        self.breaker.as_ref().map(|breaker| breaker.state())
    }

    pub async fn get_product(&self, id: Uuid) -> Result<Option<CatalogProduct>, CatalogError> {
        let outcome = match &self.breaker {
            Some(breaker) => breaker
                .call(|| self.retry.run(|| self.fetch_once(id)))
                .await
                .map_err(|err| match err {
                    CircuitBreakerError::Open => CatalogError::CircuitOpen,
                    CircuitBreakerError::Inner(err) => err,
                }),
            None => self.retry.run(|| self.fetch_once(id)).await,
        };

        let fetched = match outcome {
            Ok(fetched) => fetched,
            Err(err) => {
                return match self.fallback.as_ref().and_then(|cache| cache.get(&id)) {
                    Some(product) => {
                        metrics::counter!("fallback_activations_total", "source" => "catalog")
                            .increment(1);
                        tracing::warn!(product_id = %id, %err, "usando produto do cache de fallback");
                        Ok(Some(product))
                    }
                    None => Err(err),
                };
            }
        };

        match fetched {
            Fetched::Found(product) => {
                if let Some(cache) = &self.fallback {
                    cache.insert(id, product.clone());
                }
                Ok(Some(product))
            }
            Fetched::NotFound => {
                if let Some(cache) = &self.fallback {
                    cache.invalidate(&id);
                }
                Ok(None)
            }
            Fetched::Rejected(status) => Err(CatalogError::Unexpected(status)),
        }
    }

    async fn fetch_once(&self, id: Uuid) -> Result<Fetched, CatalogError> {
        let result = self.request(id).await;
        metrics::counter!("catalog_client_requests_total", "outcome" => attempt_outcome(&result))
            .increment(1);
        result
    }

    async fn request(&self, id: Uuid) -> Result<Fetched, CatalogError> {
        let response = self
            .http
            .get(format!("{}/products/{id}", self.base_url))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => Ok(Fetched::Found(response.json().await?)),
            StatusCode::NOT_FOUND => Ok(Fetched::NotFound),
            status if status.is_server_error() => Err(CatalogError::Unexpected(status)),
            status => Ok(Fetched::Rejected(status)),
        }
    }
}

fn attempt_outcome(result: &Result<Fetched, CatalogError>) -> &'static str {
    match result {
        Ok(Fetched::Found(_)) => "ok",
        Ok(Fetched::NotFound) => "not_found",
        Ok(Fetched::Rejected(_)) => "rejected",
        Err(CatalogError::Unexpected(_)) => "server_error",
        Err(CatalogError::Unavailable(err)) if err.is_timeout() => "timeout",
        Err(CatalogError::Unavailable(err)) if err.is_connect() => "connect_error",
        Err(_) => "other_error",
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
    fn resiliencia_usa_defaults_quando_ambiente_vazio() {
        let cfg = CatalogResilience::from_source(&vars(&[])).unwrap();

        assert_eq!(cfg.timeout, Duration::from_millis(1_000));
        assert_eq!(cfg.retry.max_attempts, 3);
        assert_eq!(cfg.breaker_failure_threshold, 5);
        assert_eq!(cfg.breaker_open_timeout, Duration::from_secs(10));
        assert_eq!(cfg.cache_ttl, Duration::from_secs(600));
        assert_eq!(cfg.cache_max_entries, 10_000);
        assert!(cfg.timeout_enabled && cfg.retry_enabled);
        assert!(cfg.breaker_enabled && cfg.fallback_enabled);
    }

    #[test]
    fn mecanismos_podem_ser_desligados_individualmente() {
        let cfg = CatalogResilience::from_source(&vars(&[
            ("CATALOG_TIMEOUT_ENABLED", "false"),
            ("CATALOG_RETRY_ENABLED", "false"),
            ("CATALOG_BREAKER_ENABLED", "false"),
            ("CATALOG_FALLBACK_ENABLED", "false"),
        ]))
        .unwrap();

        assert!(!cfg.timeout_enabled && !cfg.retry_enabled);
        assert!(!cfg.breaker_enabled && !cfg.fallback_enabled);
    }

    #[test]
    fn flag_invalida_retorna_erro() {
        let err =
            CatalogResilience::from_source(&vars(&[("CATALOG_RETRY_ENABLED", "sim")])).unwrap_err();

        assert!(err.to_string().contains("CATALOG_RETRY_ENABLED"));
    }

    #[test]
    fn resiliencia_respeita_overrides() {
        let cfg = CatalogResilience::from_source(&vars(&[
            ("CATALOG_TIMEOUT_MS", "250"),
            ("CATALOG_RETRY_ATTEMPTS", "1"),
            ("CATALOG_BREAKER_FAILURE_THRESHOLD", "2"),
        ]))
        .unwrap();

        assert_eq!(cfg.timeout, Duration::from_millis(250));
        assert_eq!(cfg.retry.max_attempts, 1);
        assert_eq!(cfg.breaker_failure_threshold, 2);
    }
}
