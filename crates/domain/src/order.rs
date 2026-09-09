use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use crate::error::DomainError;
use crate::money::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderStatus {
    Pending,
    StockReserved,
    PaymentPending,
    Confirmed,
    Cancelled,
}

impl OrderStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::StockReserved => "STOCK_RESERVED",
            Self::PaymentPending => "PAYMENT_PENDING",
            Self::Confirmed => "CONFIRMED",
            Self::Cancelled => "CANCELLED",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Confirmed | Self::Cancelled)
    }

    pub const fn can_transition_to(self, next: OrderStatus) -> bool {
        use OrderStatus::*;
        matches!(
            (self, next),
            (Pending, StockReserved)
                | (Pending, Cancelled)
                | (Pending, Confirmed)
                | (StockReserved, Confirmed)
                | (StockReserved, PaymentPending)
                | (StockReserved, Cancelled)
                | (PaymentPending, Confirmed)
                | (PaymentPending, Cancelled)
        )
    }
}

impl fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for OrderStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PENDING" => Ok(Self::Pending),
            "STOCK_RESERVED" => Ok(Self::StockReserved),
            "PAYMENT_PENDING" => Ok(Self::PaymentPending),
            "CONFIRMED" => Ok(Self::Confirmed),
            "CANCELLED" => Ok(Self::Cancelled),
            other => Err(format!("status de pedido desconhecido: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderItem {
    pub product_id: Uuid,
    pub quantity: u32,
    pub unit_price: Money,
}

impl OrderItem {
    pub fn new(product_id: Uuid, quantity: u32, unit_price: Money) -> Result<Self, DomainError> {
        if quantity == 0 {
            return Err(DomainError::ZeroQuantity);
        }
        Ok(Self {
            product_id,
            quantity,
            unit_price,
        })
    }

    pub fn subtotal(&self) -> Result<Money, DomainError> {
        self.unit_price.checked_mul(self.quantity)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub buyer_id: Uuid,
    pub items: Vec<OrderItem>,
    pub total: Money,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Order {
    pub fn new(buyer_id: Uuid, items: Vec<OrderItem>) -> Result<Self, DomainError> {
        if items.is_empty() {
            return Err(DomainError::EmptyOrder);
        }

        let mut seen = std::collections::HashSet::new();
        if let Some(dup) = items.iter().find(|item| !seen.insert(item.product_id)) {
            return Err(DomainError::DuplicateItem {
                product_id: dup.product_id,
            });
        }

        let total = items
            .iter()
            .try_fold(Money::ZERO, |acc, item| acc.checked_add(item.subtotal()?))?;

        let now = crate::time::now();
        Ok(Self {
            id: Uuid::new_v4(),
            buyer_id,
            items,
            total,
            status: OrderStatus::Pending,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn mark_stock_reserved(&mut self) -> Result<(), DomainError> {
        self.transition(OrderStatus::StockReserved)
    }

    pub fn mark_payment_pending(&mut self) -> Result<(), DomainError> {
        self.transition(OrderStatus::PaymentPending)
    }

    pub fn confirm(&mut self) -> Result<(), DomainError> {
        self.transition(OrderStatus::Confirmed)
    }

    pub fn cancel(&mut self) -> Result<(), DomainError> {
        self.transition(OrderStatus::Cancelled)
    }

    fn transition(&mut self, next: OrderStatus) -> Result<(), DomainError> {
        if !self.status.can_transition_to(next) {
            return Err(DomainError::InvalidTransition {
                from: self.status.as_str(),
                to: next.as_str(),
            });
        }
        self.status = next;
        self.updated_at = crate::time::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(quantity: u32, unit_cents: i64) -> OrderItem {
        OrderItem::new(Uuid::new_v4(), quantity, Money::from_cents(unit_cents)).unwrap()
    }

    fn order() -> Order {
        Order::new(Uuid::new_v4(), vec![item(2, 1_000), item(1, 500)]).unwrap()
    }

    #[test]
    fn calcula_total_somando_subtotais() {
        let order = order();

        assert_eq!(order.total, Money::from_cents(2_500));
        assert_eq!(order.status, OrderStatus::Pending);
    }

    #[test]
    fn rejeita_pedido_sem_itens_ou_com_quantidade_zero() {
        assert_eq!(
            Order::new(Uuid::new_v4(), vec![]).unwrap_err(),
            DomainError::EmptyOrder
        );
        assert_eq!(
            OrderItem::new(Uuid::new_v4(), 0, Money::from_cents(1)).unwrap_err(),
            DomainError::ZeroQuantity
        );
    }

    #[test]
    fn rejeita_produto_repetido() {
        let product_id = Uuid::new_v4();
        let items = vec![
            OrderItem::new(product_id, 1, Money::from_cents(100)).unwrap(),
            OrderItem::new(product_id, 2, Money::from_cents(100)).unwrap(),
        ];

        assert_eq!(
            Order::new(Uuid::new_v4(), items).unwrap_err(),
            DomainError::DuplicateItem { product_id }
        );
    }

    #[test]
    fn status_faz_round_trip_por_string() {
        for status in [
            OrderStatus::Pending,
            OrderStatus::StockReserved,
            OrderStatus::PaymentPending,
            OrderStatus::Confirmed,
            OrderStatus::Cancelled,
        ] {
            assert_eq!(status.as_str().parse::<OrderStatus>(), Ok(status));
        }
        assert!("PAID".parse::<OrderStatus>().is_err());
    }

    #[test]
    fn total_com_overflow_retorna_erro() {
        let err = Order::new(Uuid::new_v4(), vec![item(2, i64::MAX)]).unwrap_err();

        assert_eq!(err, DomainError::MoneyOverflow);
    }

    #[test]
    fn caminho_feliz_da_saga() {
        let mut order = order();

        order.mark_stock_reserved().unwrap();
        order.confirm().unwrap();

        assert_eq!(order.status, OrderStatus::Confirmed);
        assert!(order.status.is_terminal());
    }

    #[test]
    fn caminho_com_fallback_de_pagamento() {
        let mut order = order();

        order.mark_stock_reserved().unwrap();
        order.mark_payment_pending().unwrap();
        order.confirm().unwrap();

        assert_eq!(order.status, OrderStatus::Confirmed);
    }

    #[test]
    fn pode_cancelar_de_qualquer_estado_nao_terminal() {
        for setup in [
            |_: &mut Order| {},
            |o: &mut Order| o.mark_stock_reserved().unwrap(),
            |o: &mut Order| {
                o.mark_stock_reserved().unwrap();
                o.mark_payment_pending().unwrap();
            },
        ] {
            let mut order = order();
            setup(&mut order);

            order.cancel().unwrap();

            assert_eq!(order.status, OrderStatus::Cancelled);
        }
    }

    #[test]
    fn estados_terminais_nao_transitam() {
        let mut order = order();
        order.cancel().unwrap();

        assert_eq!(
            order.confirm().unwrap_err(),
            DomainError::InvalidTransition {
                from: "CANCELLED",
                to: "CONFIRMED"
            }
        );
        assert!(order.mark_stock_reserved().is_err());
    }

    #[test]
    fn confirma_diretamente_de_pending() {
        let mut order = order();

        order.confirm().unwrap();

        assert_eq!(order.status, OrderStatus::Confirmed);
    }

    #[test]
    fn status_serializa_em_screaming_snake_case() {
        let json = serde_json::to_string(&OrderStatus::StockReserved).unwrap();
        assert_eq!(json, "\"STOCK_RESERVED\"");

        let back: OrderStatus = serde_json::from_str("\"PAYMENT_PENDING\"").unwrap();
        assert_eq!(back, OrderStatus::PaymentPending);
        assert_eq!(back.to_string(), "PAYMENT_PENDING");
    }
}
