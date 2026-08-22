use chrono::{DateTime, Utc};
use domain::{Money, Payment, PaymentStatus};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(FromRow)]
struct PaymentRow {
    id: Uuid,
    order_id: Uuid,
    amount_cents: i64,
    status: String,
    failure_reason: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<PaymentRow> for Payment {
    type Error = sqlx::Error;

    fn try_from(row: PaymentRow) -> Result<Self, Self::Error> {
        let status = row
            .status
            .parse::<PaymentStatus>()
            .map_err(|e| sqlx::Error::Decode(e.into()))?;
        Ok(Self {
            id: row.id,
            order_id: row.order_id,
            amount: Money::from_cents(row.amount_cents),
            status,
            failure_reason: row.failure_reason,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

const COLUMNS: &str = "id, order_id, amount_cents, status, failure_reason, created_at, updated_at";

#[derive(Clone)]
pub struct PaymentRepository {
    pool: PgPool,
}

impl PaymentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, payment: &Payment) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO payments \
             (id, order_id, amount_cents, status, failure_reason, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(payment.id)
        .bind(payment.order_id)
        .bind(payment.amount.cents())
        .bind(payment.status.as_str())
        .bind(&payment.failure_reason)
        .bind(payment.created_at)
        .bind(payment.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_status(&self, payment: &Payment) -> sqlx::Result<bool> {
        let result = sqlx::query(
            "UPDATE payments SET status = $2, failure_reason = $3, updated_at = $4 WHERE id = $1",
        )
        .bind(payment.id)
        .bind(payment.status.as_str())
        .bind(&payment.failure_reason)
        .bind(payment.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn find_by_id(&self, id: Uuid) -> sqlx::Result<Option<Payment>> {
        sqlx::query_as::<_, PaymentRow>(&format!("SELECT {COLUMNS} FROM payments WHERE id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(Payment::try_from)
            .transpose()
    }

    pub async fn find_by_order(&self, order_id: Uuid) -> sqlx::Result<Option<Payment>> {
        sqlx::query_as::<_, PaymentRow>(&format!(
            "SELECT {COLUMNS} FROM payments WHERE order_id = $1"
        ))
        .bind(order_id)
        .fetch_optional(&self.pool)
        .await?
        .map(Payment::try_from)
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test]
    async fn insere_liquida_e_le_de_volta(pool: PgPool) {
        let repo = PaymentRepository::new(pool);
        let mut payment = Payment::new(Uuid::new_v4(), 2_500).unwrap();

        repo.insert(&payment).await.unwrap();
        assert_eq!(
            repo.find_by_id(payment.id).await.unwrap(),
            Some(payment.clone())
        );

        payment.fail("cartão recusado").unwrap();
        assert!(repo.update_status(&payment).await.unwrap());

        let found = repo.find_by_order(payment.order_id).await.unwrap().unwrap();
        assert_eq!(found.status, PaymentStatus::Failed);
        assert_eq!(found.failure_reason.as_deref(), Some("cartão recusado"));
        assert_eq!(found, payment);
    }

    #[sqlx::test]
    async fn pedido_so_pode_ter_um_pagamento(pool: PgPool) {
        let repo = PaymentRepository::new(pool);
        let order_id = Uuid::new_v4();
        repo.insert(&Payment::new(order_id, 100).unwrap())
            .await
            .unwrap();

        let err = repo
            .insert(&Payment::new(order_id, 200).unwrap())
            .await
            .unwrap_err();

        assert!(err
            .as_database_error()
            .is_some_and(|e| e.is_unique_violation()));
    }
}
