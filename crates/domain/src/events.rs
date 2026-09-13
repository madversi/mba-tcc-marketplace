use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::money::Money;
use crate::order::OrderItem;

pub trait Event: Serialize + DeserializeOwned + Send + Sync {
    const ROUTING_KEY: &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderCreated {
    pub order_id: Uuid,
    pub buyer_id: Uuid,
    pub items: Vec<OrderItem>,
    pub total: Money,
    pub occurred_at: DateTime<Utc>,
}

impl Event for OrderCreated {
    const ROUTING_KEY: &'static str = "order.created";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StockReserved {
    pub order_id: Uuid,
    pub amount: Money,
    pub occurred_at: DateTime<Utc>,
}

impl Event for StockReserved {
    const ROUTING_KEY: &'static str = "stock.reserved";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StockRejected {
    pub order_id: Uuid,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}

impl Event for StockRejected {
    const ROUTING_KEY: &'static str = "stock.rejected";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentApproved {
    pub order_id: Uuid,
    pub payment_id: Uuid,
    pub occurred_at: DateTime<Utc>,
}

impl Event for PaymentApproved {
    const ROUTING_KEY: &'static str = "payment.approved";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentFailed {
    pub order_id: Uuid,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}

impl Event for PaymentFailed {
    const ROUTING_KEY: &'static str = "payment.failed";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentPending {
    pub order_id: Uuid,
    pub payment_id: Uuid,
    pub occurred_at: DateTime<Utc>,
}

impl Event for PaymentPending {
    const ROUTING_KEY: &'static str = "payment.pending";
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip<E: Event + PartialEq + std::fmt::Debug>(event: &E) {
        let bytes = serde_json::to_vec(event).unwrap();
        let back: E = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(&back, event);
    }

    #[test]
    fn todos_os_eventos_fazem_round_trip_por_json() {
        let now = crate::time::now();
        let order_id = Uuid::new_v4();

        round_trip(&OrderCreated {
            order_id,
            buyer_id: Uuid::new_v4(),
            items: vec![OrderItem::new(Uuid::new_v4(), 2, Money::from_cents(1_000)).unwrap()],
            total: Money::from_cents(2_000),
            occurred_at: now,
        });
        round_trip(&StockReserved {
            order_id,
            amount: Money::from_cents(2_000),
            occurred_at: now,
        });
        round_trip(&StockRejected {
            order_id,
            reason: "estoque insuficiente".to_owned(),
            occurred_at: now,
        });
        round_trip(&PaymentApproved {
            order_id,
            payment_id: Uuid::new_v4(),
            occurred_at: now,
        });
        round_trip(&PaymentFailed {
            order_id,
            reason: "cartão recusado".to_owned(),
            occurred_at: now,
        });
        round_trip(&PaymentPending {
            order_id,
            payment_id: Uuid::new_v4(),
            occurred_at: now,
        });
    }

    #[test]
    fn routing_keys_sao_estaveis() {
        assert_eq!(OrderCreated::ROUTING_KEY, "order.created");
        assert_eq!(StockReserved::ROUTING_KEY, "stock.reserved");
        assert_eq!(StockRejected::ROUTING_KEY, "stock.rejected");
        assert_eq!(PaymentApproved::ROUTING_KEY, "payment.approved");
        assert_eq!(PaymentFailed::ROUTING_KEY, "payment.failed");
        assert_eq!(PaymentPending::ROUTING_KEY, "payment.pending");
    }

    #[test]
    fn formato_json_do_evento_e_o_esperado() {
        let event = StockReserved {
            order_id: Uuid::nil(),
            amount: Money::from_cents(2_500),
            occurred_at: "2026-09-10T12:00:00Z".parse().unwrap(),
        };

        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "order_id": "00000000-0000-0000-0000-000000000000",
                "amount": 2500,
                "occurred_at": "2026-09-10T12:00:00Z"
            })
        );
    }
}
