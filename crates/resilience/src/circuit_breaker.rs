use std::future::Future;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::Duration;

use tokio::time::Instant;

const CLOSED: u8 = 0;
const OPEN: u8 = 1;
const HALF_OPEN: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CircuitBreakerError<E> {
    Open,
    Inner(E),
}

impl<E: std::fmt::Display> std::fmt::Display for CircuitBreakerError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "circuito aberto: chamada rejeitada"),
            Self::Inner(err) => write!(f, "{err}"),
        }
    }
}

impl<E: std::fmt::Debug + std::fmt::Display> std::error::Error for CircuitBreakerError<E> {}

pub struct CircuitBreaker {
    name: String,
    state: AtomicU8,
    failure_count: AtomicU32,
    opened_at_millis: AtomicU64,
    created_at: Instant,
    failure_threshold: u32,
    open_timeout: Duration,
}

impl CircuitBreaker {
    pub fn new(name: impl Into<String>, failure_threshold: u32, open_timeout: Duration) -> Self {
        let breaker = Self {
            name: name.into(),
            state: AtomicU8::new(CLOSED),
            failure_count: AtomicU32::new(0),
            opened_at_millis: AtomicU64::new(0),
            created_at: Instant::now(),
            failure_threshold,
            open_timeout,
        };
        breaker.publish_state(CLOSED);
        breaker
    }

    pub fn state(&self) -> CircuitState {
        match self.state.load(Ordering::Acquire) {
            CLOSED => CircuitState::Closed,
            HALF_OPEN => CircuitState::HalfOpen,
            _ if self.open_timeout_elapsed() => CircuitState::HalfOpen,
            _ => CircuitState::Open,
        }
    }

    pub async fn call<T, E, F, Fut>(&self, operation: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        if !self.allow_call() {
            return Err(CircuitBreakerError::Open);
        }

        match operation().await {
            Ok(value) => {
                self.on_success();
                Ok(value)
            }
            Err(err) => {
                self.on_failure();
                Err(CircuitBreakerError::Inner(err))
            }
        }
    }

    fn open_timeout_elapsed(&self) -> bool {
        let opened_at = self.opened_at_millis.load(Ordering::Acquire);
        let elapsed = self.created_at.elapsed().as_millis() as u64;
        elapsed.saturating_sub(opened_at) >= self.open_timeout.as_millis() as u64
    }

    fn allow_call(&self) -> bool {
        match self.state.load(Ordering::Acquire) {
            OPEN => {
                let probing = self.open_timeout_elapsed()
                    && self
                        .state
                        .compare_exchange(OPEN, HALF_OPEN, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok();
                if probing {
                    self.record_transition(HALF_OPEN);
                }
                probing
            }
            _ => true,
        }
    }

    fn on_success(&self) {
        self.failure_count.store(0, Ordering::Release);
        if self.state.swap(CLOSED, Ordering::AcqRel) != CLOSED {
            self.record_transition(CLOSED);
        }
    }

    fn on_failure(&self) {
        if self.state.load(Ordering::Acquire) == HALF_OPEN {
            self.open();
            return;
        }

        let failures = self.failure_count.fetch_add(1, Ordering::AcqRel) + 1;
        if failures >= self.failure_threshold {
            self.open();
        }
    }

    fn open(&self) {
        let now_millis = self.created_at.elapsed().as_millis() as u64;
        self.opened_at_millis.store(now_millis, Ordering::Release);
        self.failure_count.store(0, Ordering::Release);
        if self.state.swap(OPEN, Ordering::AcqRel) != OPEN {
            self.record_transition(OPEN);
        }
    }

    fn record_transition(&self, to: u8) {
        self.publish_state(to);
        let label = match to {
            CLOSED => "closed",
            HALF_OPEN => "half_open",
            _ => "open",
        };
        metrics::counter!(
            "circuit_breaker_transitions_total",
            "breaker" => self.name.clone(),
            "to" => label,
        )
        .increment(1);
    }

    fn publish_state(&self, state: u8) {
        metrics::gauge!("circuit_breaker_state", "breaker" => self.name.clone())
            .set(f64::from(state_gauge_value(state)));
    }
}

const fn state_gauge_value(state: u8) -> u8 {
    match state {
        CLOSED => 0,
        HALF_OPEN => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn comeca_fechado() {
        let breaker = CircuitBreaker::new("teste", 2, Duration::from_secs(30));
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn abre_apos_atingir_o_limite_de_falhas() {
        let breaker = CircuitBreaker::new("teste", 2, Duration::from_secs(30));

        let _ = breaker.call(|| async { Err::<(), _>("falha") }).await;
        assert_eq!(breaker.state(), CircuitState::Closed);

        let _ = breaker.call(|| async { Err::<(), _>("falha") }).await;
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn sucesso_reseta_a_contagem_de_falhas() {
        let breaker = CircuitBreaker::new("teste", 2, Duration::from_secs(30));

        let _ = breaker.call(|| async { Err::<(), _>("falha") }).await;
        let _ = breaker.call(|| async { Ok::<_, &str>(()) }).await;
        let _ = breaker.call(|| async { Err::<(), _>("falha") }).await;

        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn chamada_e_rejeitada_enquanto_aberto_sem_executar_a_operacao() {
        let breaker = CircuitBreaker::new("teste", 1, Duration::from_secs(30));
        let _ = breaker.call(|| async { Err::<(), _>("falha") }).await;
        assert_eq!(breaker.state(), CircuitState::Open);

        let attempts = Cell::new(0);
        let result = breaker
            .call(|| {
                attempts.set(attempts.get() + 1);
                async { Ok::<_, &str>(1) }
            })
            .await;

        assert_eq!(result, Err(CircuitBreakerError::Open));
        assert_eq!(attempts.get(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn permite_chamada_de_teste_apos_o_timeout_e_fecha_com_sucesso() {
        let breaker = CircuitBreaker::new("teste", 1, Duration::from_secs(30));
        let _ = breaker.call(|| async { Err::<(), _>("falha") }).await;

        tokio::time::advance(Duration::from_secs(31)).await;

        let result = breaker.call(|| async { Ok::<_, &str>(42) }).await;

        assert_eq!(result, Ok(42));
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn falha_durante_o_teste_reabre_o_circuito() {
        let breaker = CircuitBreaker::new("teste", 1, Duration::from_secs(30));
        let _ = breaker.call(|| async { Err::<(), _>("falha") }).await;

        tokio::time::advance(Duration::from_secs(31)).await;

        let result = breaker
            .call(|| async { Err::<i32, _>("falha de novo") })
            .await;

        assert_eq!(result, Err(CircuitBreakerError::Inner("falha de novo")));
        assert_eq!(breaker.state(), CircuitState::Open);
    }
}
