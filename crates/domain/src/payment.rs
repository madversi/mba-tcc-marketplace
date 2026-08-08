use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;
use crate::money::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaymentStatus {
    Pending,
    Approved,
    Failed,
}

impl PaymentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Approved => "APPROVED",
            Self::Failed => "FAILED",
        }
    }
}

impl fmt::Display for PaymentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    pub id: Uuid,
    pub order_id: Uuid,
    pub amount: Money,
    pub status: PaymentStatus,
    pub failure_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Payment {
    pub fn new(order_id: Uuid, amount_cents: i64) -> Result<Self, DomainError> {
        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            order_id,
            amount: Money::positive(amount_cents)?,
            status: PaymentStatus::Pending,
            failure_reason: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn approve(&mut self) -> Result<(), DomainError> {
        self.settle(PaymentStatus::Approved, None)
    }

    pub fn fail(&mut self, reason: impl Into<String>) -> Result<(), DomainError> {
        self.settle(PaymentStatus::Failed, Some(reason.into()))
    }

    fn settle(&mut self, next: PaymentStatus, reason: Option<String>) -> Result<(), DomainError> {
        if self.status != PaymentStatus::Pending {
            return Err(DomainError::InvalidTransition {
                from: self.status.as_str(),
                to: next.as_str(),
            });
        }
        self.status = next;
        self.failure_reason = reason;
        self.updated_at = Utc::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payment() -> Payment {
        Payment::new(Uuid::new_v4(), 2_500).unwrap()
    }

    #[test]
    fn nasce_pendente_com_valor_positivo() {
        let payment = payment();

        assert_eq!(payment.status, PaymentStatus::Pending);
        assert_eq!(payment.amount, Money::from_cents(2_500));
        assert_eq!(
            Payment::new(Uuid::new_v4(), 0).unwrap_err(),
            DomainError::NonPositiveAmount(0)
        );
    }

    #[test]
    fn aprova_pagamento_pendente() {
        let mut payment = payment();

        payment.approve().unwrap();

        assert_eq!(payment.status, PaymentStatus::Approved);
        assert_eq!(payment.failure_reason, None);
    }

    #[test]
    fn falha_registra_motivo() {
        let mut payment = payment();

        payment.fail("cartão recusado").unwrap();

        assert_eq!(payment.status, PaymentStatus::Failed);
        assert_eq!(payment.failure_reason.as_deref(), Some("cartão recusado"));
    }

    #[test]
    fn pagamento_liquidado_nao_muda_mais() {
        let mut payment = payment();
        payment.approve().unwrap();

        assert_eq!(
            payment.fail("tarde demais").unwrap_err(),
            DomainError::InvalidTransition {
                from: "APPROVED",
                to: "FAILED"
            }
        );
        assert!(payment.approve().is_err());
    }
}
