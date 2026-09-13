use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use domain::Payment;
use resilience::{CircuitBreaker, CircuitBreakerError, CircuitState};
use shared::config::{env_vars, optional, parse_or};
use shared::{ConfigError, EnvVars};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChargeOutcome {
    Approved,
    Declined { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("gateway de pagamento indisponível")]
    Unavailable,
    #[error("gateway de pagamento indisponível: circuito aberto")]
    CircuitOpen,
}

#[derive(Debug, serde::Serialize)]
pub struct GatewayConfigView {
    pub failure_rate: f64,
    pub latency_ms: u64,
    pub unavailable: bool,
}

#[derive(Default)]
struct GatewayConfig {
    failure_rate_bp: AtomicU32,
    latency_ms: AtomicU64,
    unavailable: AtomicBool,
}

fn to_basis_points(rate: f64) -> u32 {
    (rate.clamp(0.0, 1.0) * 10_000.0).round() as u32
}

#[derive(Clone, Default)]
pub struct SimulatedGateway {
    config: Arc<GatewayConfig>,
}

impl SimulatedGateway {
    pub fn from_env() -> Self {
        let vars = env_vars();
        let failure_rate = parse_or(&vars, "FAILURE_RATE", 0.0).unwrap_or(0.0);
        let latency_ms = parse_or(&vars, "LATENCY_MS", 0).unwrap_or(0);
        let unavailable = optional(&vars, "UNAVAILABLE") == Some("true");

        let gateway = Self::default();
        gateway.set_failure_rate(failure_rate);
        gateway.set_latency_ms(latency_ms);
        gateway.set_unavailable(unavailable);
        gateway
    }

    pub async fn charge(&self, _payment: &Payment) -> Result<ChargeOutcome, GatewayError> {
        let latency = self.config.latency_ms.load(Ordering::Relaxed);
        if latency > 0 {
            tokio::time::sleep(Duration::from_millis(latency)).await;
        }

        if self.config.unavailable.load(Ordering::Relaxed) {
            return Err(GatewayError::Unavailable);
        }

        let rate = self.config.failure_rate_bp.load(Ordering::Relaxed) as f64 / 10_000.0;
        if rate > 0.0 && rand::random::<f64>() < rate {
            return Err(GatewayError::Unavailable);
        }

        Ok(ChargeOutcome::Approved)
    }

    pub fn set_failure_rate(&self, rate: f64) {
        self.config
            .failure_rate_bp
            .store(to_basis_points(rate), Ordering::Relaxed);
    }

    pub fn set_latency_ms(&self, latency_ms: u64) {
        self.config.latency_ms.store(latency_ms, Ordering::Relaxed);
    }

    pub fn set_unavailable(&self, unavailable: bool) {
        self.config
            .unavailable
            .store(unavailable, Ordering::Relaxed);
    }

    pub fn config_view(&self) -> GatewayConfigView {
        GatewayConfigView {
            failure_rate: self.config.failure_rate_bp.load(Ordering::Relaxed) as f64 / 10_000.0,
            latency_ms: self.config.latency_ms.load(Ordering::Relaxed),
            unavailable: self.config.unavailable.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GatewayResilience {
    pub breaker_failure_threshold: u32,
    pub breaker_open_timeout: Duration,
}

impl GatewayResilience {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_source(&env_vars())
    }

    pub fn from_source(vars: &EnvVars) -> Result<Self, ConfigError> {
        Ok(Self {
            breaker_failure_threshold: parse_or(vars, "GATEWAY_BREAKER_FAILURE_THRESHOLD", 5)?,
            breaker_open_timeout: Duration::from_millis(parse_or(
                vars,
                "GATEWAY_BREAKER_OPEN_TIMEOUT_MS",
                10_000,
            )?),
        })
    }
}

#[derive(Clone)]
pub struct GatewayClient {
    gateway: SimulatedGateway,
    breaker: Arc<CircuitBreaker>,
}

impl GatewayClient {
    pub fn new(gateway: SimulatedGateway, resilience: GatewayResilience) -> Self {
        Self {
            gateway,
            breaker: Arc::new(CircuitBreaker::new(
                "payment_gateway",
                resilience.breaker_failure_threshold,
                resilience.breaker_open_timeout,
            )),
        }
    }

    pub fn simulated(&self) -> &SimulatedGateway {
        &self.gateway
    }

    pub fn circuit_state(&self) -> CircuitState {
        self.breaker.state()
    }

    pub async fn charge(&self, payment: &Payment) -> Result<ChargeOutcome, GatewayError> {
        self.breaker
            .call(|| self.gateway.charge(payment))
            .await
            .map_err(|err| match err {
                CircuitBreakerError::Open => GatewayError::CircuitOpen,
                CircuitBreakerError::Inner(err) => err,
            })
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn payment() -> Payment {
        Payment::new(Uuid::new_v4(), 1_000).unwrap()
    }

    #[tokio::test]
    async fn por_padrao_sempre_aprova() {
        let gateway = SimulatedGateway::default();
        let result = gateway.charge(&payment()).await;
        assert!(matches!(result, Ok(ChargeOutcome::Approved)));
    }

    #[tokio::test]
    async fn unavailable_forca_falha_mesmo_com_failure_rate_zero() {
        let gateway = SimulatedGateway::default();
        gateway.set_unavailable(true);

        let result = gateway.charge(&payment()).await;
        assert!(matches!(result, Err(GatewayError::Unavailable)));
    }

    #[tokio::test]
    async fn failure_rate_um_sempre_falha() {
        let gateway = SimulatedGateway::default();
        gateway.set_failure_rate(1.0);

        let result = gateway.charge(&payment()).await;
        assert!(matches!(result, Err(GatewayError::Unavailable)));
    }

    #[test]
    fn config_view_reflete_os_valores_atuais() {
        let gateway = SimulatedGateway::default();
        gateway.set_failure_rate(0.25);
        gateway.set_latency_ms(50);
        gateway.set_unavailable(true);

        let view = gateway.config_view();
        assert_eq!(view.failure_rate, 0.25);
        assert_eq!(view.latency_ms, 50);
        assert!(view.unavailable);
    }

    #[tokio::test]
    async fn cliente_abre_o_circuito_e_recusa_sem_chamar_o_gateway() {
        let gateway = SimulatedGateway::default();
        let client = GatewayClient::new(
            gateway.clone(),
            GatewayResilience {
                breaker_failure_threshold: 2,
                breaker_open_timeout: Duration::from_secs(30),
            },
        );

        gateway.set_unavailable(true);
        for _ in 0..2 {
            let result = client.charge(&payment()).await;
            assert!(matches!(result, Err(GatewayError::Unavailable)));
        }
        assert_eq!(client.circuit_state(), CircuitState::Open);

        gateway.set_unavailable(false);
        let result = client.charge(&payment()).await;
        assert!(matches!(result, Err(GatewayError::CircuitOpen)));
    }

    #[test]
    fn failure_rate_e_limitado_entre_zero_e_um() {
        assert_eq!(to_basis_points(-1.0), 0);
        assert_eq!(to_basis_points(2.0), 10_000);
    }
}
