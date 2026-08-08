use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Money = Money(0);

    pub const fn from_cents(cents: i64) -> Self {
        Self(cents)
    }

    pub fn positive(cents: i64) -> Result<Self, DomainError> {
        if cents <= 0 {
            return Err(DomainError::NonPositiveAmount(cents));
        }
        Ok(Self(cents))
    }

    pub const fn cents(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Money) -> Result<Money, DomainError> {
        self.0
            .checked_add(other.0)
            .map(Money)
            .ok_or(DomainError::MoneyOverflow)
    }

    pub fn checked_mul(self, factor: u32) -> Result<Money, DomainError> {
        self.0
            .checked_mul(i64::from(factor))
            .map(Money)
            .ok_or(DomainError::MoneyOverflow)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        write!(f, "{sign}R$ {},{:02}", abs / 100, abs % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positivo_rejeita_zero_e_negativo() {
        assert_eq!(Money::positive(0), Err(DomainError::NonPositiveAmount(0)));
        assert_eq!(Money::positive(-5), Err(DomainError::NonPositiveAmount(-5)));
        assert_eq!(Money::positive(1).unwrap().cents(), 1);
    }

    #[test]
    fn soma_e_multiplicacao_com_overflow_retornam_erro() {
        let max = Money::from_cents(i64::MAX);

        assert_eq!(
            max.checked_add(Money::from_cents(1)),
            Err(DomainError::MoneyOverflow)
        );
        assert_eq!(max.checked_mul(2), Err(DomainError::MoneyOverflow));
        assert_eq!(
            Money::from_cents(1050).checked_mul(3).unwrap(),
            Money::from_cents(3150)
        );
    }

    #[test]
    fn formata_em_reais() {
        assert_eq!(Money::from_cents(123456).to_string(), "R$ 1234,56");
        assert_eq!(Money::from_cents(5).to_string(), "R$ 0,05");
        assert_eq!(Money::from_cents(-250).to_string(), "-R$ 2,50");
    }

    #[test]
    fn serializa_como_inteiro() {
        let json = serde_json::to_string(&Money::from_cents(999)).unwrap();
        assert_eq!(json, "999");

        let back: Money = serde_json::from_str("999").unwrap();
        assert_eq!(back, Money::from_cents(999));
    }
}
