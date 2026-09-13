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
    pub timeout: Duration,
    pub retry: RetryPolicy,
    pub breaker_failure_threshold: u32,
    pub breaker_open_timeout: Duration,
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
            timeout: ms("CATALOG_TIMEOUT_MS", 1_000)?,
            retry: RetryPolicy::new(
                parse_or(vars, "CATALOG_RETRY_ATTEMPTS", 3)?,
                ms("CATALOG_RETRY_BASE_DELAY_MS", 100)?,
                ms("CATALOG_RETRY_MAX_DELAY_MS", 1_000)?,
            ),
            breaker_failure_threshold: parse_or(vars, "CATALOG_BREAKER_FAILURE_THRESHOLD", 5)?,
            breaker_open_timeout: ms("CATALOG_BREAKER_OPEN_TIMEOUT_MS", 10_000)?,
            cache_ttl: Duration::from_secs(parse_or(vars, "CATALOG_CACHE_TTL_SECS", 600)?),
            cache_max_entries: parse_or(vars, "CATALOG_CACHE_MAX_ENTRIES", 10_000)?,
        })
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
    breaker: Arc<CircuitBreaker>,
    fallback: Cache<Uuid, CatalogProduct>,
}

impl CatalogClient {
    pub fn new(base_url: impl Into<String>, resilience: CatalogResilience) -> Self {
        let http = reqwest::Client::builder()
            .timeout(resilience.timeout)
            .build()
            .expect("configuração válida do cliente HTTP do catálogo");

        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            http,
            retry: resilience.retry,
            breaker: Arc::new(CircuitBreaker::new(
                "catalog",
                resilience.breaker_failure_threshold,
                resilience.breaker_open_timeout,
            )),
            fallback: Cache::builder()
                .max_capacity(resilience.cache_max_entries)
                .time_to_live(resilience.cache_ttl)
                .build(),
        }
    }

    pub fn circuit_state(&self) -> CircuitState {
        self.breaker.state()
    }

    pub async fn get_product(&self, id: Uuid) -> Result<Option<CatalogProduct>, CatalogError> {
        let outcome = self
            .breaker
            .call(|| self.retry.run(|| self.fetch_once(id)))
            .await;

        let fetched = match outcome {
            Ok(fetched) => fetched,
            Err(err) => {
                let err = match err {
                    CircuitBreakerError::Open => CatalogError::CircuitOpen,
                    CircuitBreakerError::Inner(err) => err,
                };
                return match self.fallback.get(&id) {
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
                self.fallback.insert(id, product.clone());
                Ok(Some(product))
            }
            Fetched::NotFound => {
                self.fallback.invalidate(&id);
                Ok(None)
            }
            Fetched::Rejected(status) => Err(CatalogError::Unexpected(status)),
        }
    }

    async fn fetch_once(&self, id: Uuid) -> Result<Fetched, CatalogError> {
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
