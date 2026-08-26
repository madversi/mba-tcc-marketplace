use chrono::{DateTime, Utc};
use domain::{Money, Order, OrderItem, OrderStatus};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(FromRow)]
struct OrderRow {
    id: Uuid,
    buyer_id: Uuid,
    status: String,
    total_cents: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct ItemRow {
    product_id: Uuid,
    quantity: i64,
    unit_price_cents: i64,
}

impl TryFrom<ItemRow> for OrderItem {
    type Error = sqlx::Error;

    fn try_from(row: ItemRow) -> Result<Self, Self::Error> {
        Ok(Self {
            product_id: row.product_id,
            quantity: u32::try_from(row.quantity).map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
            unit_price: Money::from_cents(row.unit_price_cents),
        })
    }
}

fn build_order(row: OrderRow, items: Vec<ItemRow>) -> sqlx::Result<Order> {
    let status = row
        .status
        .parse::<OrderStatus>()
        .map_err(|e| sqlx::Error::Decode(e.into()))?;
    let items = items
        .into_iter()
        .map(OrderItem::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Order {
        id: row.id,
        buyer_id: row.buyer_id,
        items,
        total: Money::from_cents(row.total_cents),
        status,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

#[derive(Clone)]
pub struct OrderRepository {
    pool: PgPool,
}

impl OrderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, order: &Order) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO orders (id, buyer_id, status, total_cents, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(order.id)
        .bind(order.buyer_id)
        .bind(order.status.as_str())
        .bind(order.total.cents())
        .bind(order.created_at)
        .bind(order.updated_at)
        .execute(&mut *tx)
        .await?;

        for item in &order.items {
            sqlx::query(
                "INSERT INTO order_items (order_id, product_id, quantity, unit_price_cents) \
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(order.id)
            .bind(item.product_id)
            .bind(i64::from(item.quantity))
            .bind(item.unit_price.cents())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await
    }

    pub async fn find_by_id(&self, id: Uuid) -> sqlx::Result<Option<Order>> {
        let Some(row) = sqlx::query_as::<_, OrderRow>(
            "SELECT id, buyer_id, status, total_cents, created_at, updated_at \
             FROM orders WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };

        let items = sqlx::query_as::<_, ItemRow>(
            "SELECT product_id, quantity, unit_price_cents FROM order_items \
             WHERE order_id = $1 ORDER BY product_id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        build_order(row, items).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order() -> Order {
        let items = vec![
            OrderItem::new(Uuid::new_v4(), 2, Money::from_cents(1_000)).unwrap(),
            OrderItem::new(Uuid::new_v4(), 1, Money::from_cents(500)).unwrap(),
        ];
        Order::new(Uuid::new_v4(), items).unwrap()
    }

    #[sqlx::test]
    async fn insere_e_le_pedido_com_itens(pool: PgPool) {
        let repo = OrderRepository::new(pool);
        let mut order = order();
        order.items.sort_by_key(|i| i.product_id);

        repo.insert(&order).await.unwrap();
        let found = repo.find_by_id(order.id).await.unwrap();

        assert_eq!(found, Some(order));
    }

    #[sqlx::test]
    async fn pedido_inexistente_retorna_none(pool: PgPool) {
        let repo = OrderRepository::new(pool);

        assert_eq!(repo.find_by_id(Uuid::new_v4()).await.unwrap(), None);
    }
}
