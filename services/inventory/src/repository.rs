use chrono::{DateTime, Utc};
use domain::{DomainError, StockItem};
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum StockError {
    #[error("estoque não encontrado")]
    NotFound,
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[derive(FromRow)]
struct StockRow {
    product_id: Uuid,
    available: i64,
    reserved: i64,
    updated_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct ReservationRow {
    product_id: Uuid,
    quantity: i64,
}

impl TryFrom<StockRow> for StockItem {
    type Error = sqlx::Error;

    fn try_from(row: StockRow) -> Result<Self, Self::Error> {
        let to_u32 = |v: i64| u32::try_from(v).map_err(|e| sqlx::Error::Decode(Box::new(e)));
        Ok(Self {
            product_id: row.product_id,
            available: to_u32(row.available)?,
            reserved: to_u32(row.reserved)?,
            updated_at: row.updated_at,
        })
    }
}

const COLUMNS: &str = "product_id, available, reserved, updated_at";

#[derive(Clone)]
pub struct StockRepository {
    pool: PgPool,
}

impl StockRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find(&self, product_id: Uuid) -> sqlx::Result<Option<StockItem>> {
        sqlx::query_as::<_, StockRow>(&format!(
            "SELECT {COLUMNS} FROM stock WHERE product_id = $1"
        ))
        .bind(product_id)
        .fetch_optional(&self.pool)
        .await?
        .map(StockItem::try_from)
        .transpose()
    }

    pub async fn set_available(&self, product_id: Uuid, quantity: u32) -> sqlx::Result<StockItem> {
        let now = domain::time::now();
        sqlx::query_as::<_, StockRow>(&format!(
            "INSERT INTO stock (product_id, available, reserved, updated_at) \
             VALUES ($1, $2, 0, $3) \
             ON CONFLICT (product_id) DO UPDATE \
             SET available = EXCLUDED.available, updated_at = EXCLUDED.updated_at \
             RETURNING {COLUMNS}"
        ))
        .bind(product_id)
        .bind(i64::from(quantity))
        .bind(now)
        .fetch_one(&self.pool)
        .await?
        .try_into()
    }

    pub async fn modify(
        &self,
        product_id: Uuid,
        apply: impl FnOnce(&mut StockItem) -> Result<(), DomainError>,
    ) -> Result<StockItem, StockError> {
        let mut tx = self.pool.begin().await?;
        let item = Self::apply_locked(&mut tx, product_id, apply).await?;
        tx.commit().await?;
        Ok(item)
    }

    pub async fn reserve_many(
        &self,
        order_id: Uuid,
        items: &[(Uuid, u32)],
    ) -> Result<(), StockError> {
        let mut sorted = items.to_vec();
        sorted.sort_by_key(|(product_id, _)| *product_id);

        let mut tx = self.pool.begin().await?;
        for (product_id, quantity) in &sorted {
            Self::apply_locked(&mut tx, *product_id, |s| s.reserve(*quantity)).await?;
        }
        for (product_id, quantity) in &sorted {
            sqlx::query(
                "INSERT INTO stock_reservations (order_id, product_id, quantity) VALUES ($1, $2, $3)",
            )
            .bind(order_id)
            .bind(product_id)
            .bind(i64::from(*quantity))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn release_reserved(&self, order_id: Uuid) -> Result<(), StockError> {
        self.settle_reservations(order_id, |s, q| s.release(q))
            .await
    }

    pub async fn commit_reserved(&self, order_id: Uuid) -> Result<(), StockError> {
        self.settle_reservations(order_id, |s, q| s.commit(q)).await
    }

    pub async fn find_reservation(
        &self,
        order_id: Uuid,
        product_id: Uuid,
    ) -> sqlx::Result<Option<u32>> {
        let quantity: Option<i64> = sqlx::query_scalar(
            "SELECT quantity FROM stock_reservations WHERE order_id = $1 AND product_id = $2",
        )
        .bind(order_id)
        .bind(product_id)
        .fetch_optional(&self.pool)
        .await?;
        quantity
            .map(|q| u32::try_from(q).map_err(|e| sqlx::Error::Decode(Box::new(e))))
            .transpose()
    }

    async fn settle_reservations(
        &self,
        order_id: Uuid,
        apply: impl Fn(&mut StockItem, u32) -> Result<(), DomainError>,
    ) -> Result<(), StockError> {
        let mut tx = self.pool.begin().await?;

        let rows = sqlx::query_as::<_, ReservationRow>(
            "SELECT product_id, quantity FROM stock_reservations WHERE order_id = $1",
        )
        .bind(order_id)
        .fetch_all(&mut *tx)
        .await?;

        for row in &rows {
            let quantity =
                u32::try_from(row.quantity).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
            Self::apply_locked(&mut tx, row.product_id, |s| apply(s, quantity)).await?;
        }

        sqlx::query("DELETE FROM stock_reservations WHERE order_id = $1")
            .bind(order_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn apply_locked(
        conn: &mut PgConnection,
        product_id: Uuid,
        apply: impl FnOnce(&mut StockItem) -> Result<(), DomainError>,
    ) -> Result<StockItem, StockError> {
        let row = sqlx::query_as::<_, StockRow>(&format!(
            "SELECT {COLUMNS} FROM stock WHERE product_id = $1 FOR UPDATE"
        ))
        .bind(product_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(StockError::NotFound)?;
        let mut item = StockItem::try_from(row)?;

        apply(&mut item)?;

        sqlx::query(
            "UPDATE stock SET available = $2, reserved = $3, updated_at = $4 WHERE product_id = $1",
        )
        .bind(item.product_id)
        .bind(i64::from(item.available))
        .bind(i64::from(item.reserved))
        .bind(item.updated_at)
        .execute(&mut *conn)
        .await?;

        Ok(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test]
    async fn set_available_cria_e_depois_redefine(pool: PgPool) {
        let repo = StockRepository::new(pool);
        let product_id = Uuid::new_v4();

        let created = repo.set_available(product_id, 10).await.unwrap();
        assert_eq!((created.available, created.reserved), (10, 0));

        repo.modify(product_id, |s| s.reserve(4)).await.unwrap();
        let updated = repo.set_available(product_id, 20).await.unwrap();

        assert_eq!((updated.available, updated.reserved), (20, 4));
        assert_eq!(repo.find(product_id).await.unwrap(), Some(updated));
    }

    #[sqlx::test]
    async fn modify_de_produto_inexistente_da_not_found(pool: PgPool) {
        let repo = StockRepository::new(pool);

        let err = repo
            .modify(Uuid::new_v4(), |s| s.reserve(1))
            .await
            .unwrap_err();

        assert!(matches!(err, StockError::NotFound));
    }

    #[sqlx::test]
    async fn erro_de_dominio_nao_persiste_nada(pool: PgPool) {
        let repo = StockRepository::new(pool);
        let product_id = Uuid::new_v4();
        repo.set_available(product_id, 2).await.unwrap();

        let err = repo.modify(product_id, |s| s.reserve(5)).await.unwrap_err();

        assert!(matches!(
            err,
            StockError::Domain(DomainError::InsufficientStock { .. })
        ));
        let item = repo.find(product_id).await.unwrap().unwrap();
        assert_eq!((item.available, item.reserved), (2, 0));
    }

    #[sqlx::test]
    async fn reserve_many_reserva_todos_quando_ha_saldo(pool: PgPool) {
        let repo = StockRepository::new(pool);
        let order_id = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        repo.set_available(a, 10).await.unwrap();
        repo.set_available(b, 10).await.unwrap();

        repo.reserve_many(order_id, &[(a, 3), (b, 4)])
            .await
            .unwrap();

        let item_a = repo.find(a).await.unwrap().unwrap();
        let item_b = repo.find(b).await.unwrap().unwrap();
        assert_eq!((item_a.available, item_a.reserved), (7, 3));
        assert_eq!((item_b.available, item_b.reserved), (6, 4));
        assert_eq!(repo.find_reservation(order_id, a).await.unwrap(), Some(3));
        assert_eq!(repo.find_reservation(order_id, b).await.unwrap(), Some(4));
    }

    #[sqlx::test]
    async fn reserve_many_e_atomico_entre_produtos(pool: PgPool) {
        let repo = StockRepository::new(pool);
        let order_id = Uuid::new_v4();
        let ok = Uuid::new_v4();
        let short = Uuid::new_v4();
        repo.set_available(ok, 10).await.unwrap();
        repo.set_available(short, 1).await.unwrap();

        let err = repo
            .reserve_many(order_id, &[(ok, 5), (short, 5)])
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            StockError::Domain(DomainError::InsufficientStock { .. })
        ));
        let ok_item = repo.find(ok).await.unwrap().unwrap();
        assert_eq!((ok_item.available, ok_item.reserved), (10, 0));
        assert_eq!(repo.find_reservation(order_id, ok).await.unwrap(), None);
    }

    #[sqlx::test]
    async fn release_reserved_devolve_ao_disponivel_e_apaga_reserva(pool: PgPool) {
        let repo = StockRepository::new(pool);
        let order_id = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        repo.set_available(a, 10).await.unwrap();
        repo.set_available(b, 10).await.unwrap();
        repo.reserve_many(order_id, &[(a, 3), (b, 4)])
            .await
            .unwrap();

        repo.release_reserved(order_id).await.unwrap();

        let item_a = repo.find(a).await.unwrap().unwrap();
        let item_b = repo.find(b).await.unwrap().unwrap();
        assert_eq!((item_a.available, item_a.reserved), (10, 0));
        assert_eq!((item_b.available, item_b.reserved), (10, 0));
        assert_eq!(repo.find_reservation(order_id, a).await.unwrap(), None);
    }

    #[sqlx::test]
    async fn commit_reserved_consome_a_reserva_e_apaga_registro(pool: PgPool) {
        let repo = StockRepository::new(pool);
        let order_id = Uuid::new_v4();
        let a = Uuid::new_v4();
        repo.set_available(a, 10).await.unwrap();
        repo.reserve_many(order_id, &[(a, 3)]).await.unwrap();

        repo.commit_reserved(order_id).await.unwrap();

        let item_a = repo.find(a).await.unwrap().unwrap();
        assert_eq!((item_a.available, item_a.reserved), (7, 0));
        assert_eq!(repo.find_reservation(order_id, a).await.unwrap(), None);
    }

    #[sqlx::test]
    async fn settle_de_pedido_sem_reserva_e_no_op(pool: PgPool) {
        let repo = StockRepository::new(pool);

        repo.release_reserved(Uuid::new_v4()).await.unwrap();
        repo.commit_reserved(Uuid::new_v4()).await.unwrap();
    }
}
