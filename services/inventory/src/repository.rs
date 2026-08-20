use chrono::{DateTime, Utc};
use domain::{DomainError, StockItem};
use sqlx::{FromRow, PgPool};
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

        let row = sqlx::query_as::<_, StockRow>(&format!(
            "SELECT {COLUMNS} FROM stock WHERE product_id = $1 FOR UPDATE"
        ))
        .bind(product_id)
        .fetch_optional(&mut *tx)
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
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
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
}
