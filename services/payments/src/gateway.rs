use domain::Payment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChargeOutcome {
    Approved,
    Declined { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("gateway de pagamento indisponível")]
    Unavailable,
}

#[derive(Clone, Default)]
pub struct SimulatedGateway;

impl SimulatedGateway {
    pub async fn charge(&self, _payment: &Payment) -> Result<ChargeOutcome, GatewayError> {
        Ok(ChargeOutcome::Approved)
    }
}
