pub mod circuit_breaker;
pub mod retry;

pub use circuit_breaker::{CircuitBreaker, CircuitBreakerError, CircuitState};
pub use retry::RetryPolicy;

pub fn report_mechanism(mechanism: &'static str, enabled: bool) {
    metrics::gauge!("resilience_mechanism_enabled", "mechanism" => mechanism).set(if enabled {
        1.0
    } else {
        0.0
    });
    tracing::info!(mechanism, enabled, "mecanismo de resiliência configurado");
}
