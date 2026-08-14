use chrono::{DateTime, Utc};
use domain::{Money, Product, Seller};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(FromRow)]
struct SellerRow {
    id: Uuid,
    name: String,
    email: String,
    created_at: DateTime<Utc>,
}

impl From<SellerRow> for Seller {
    fn from(row: SellerRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            email: row.email,
            created_at: row.created_at,
        }
    }
}

#[derive(FromRow)]
struct ProductRow {
    id: Uuid,
    seller_id: Uuid,
    name: String,
    description: Option<String>,
    price_cents: i64,
    active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ProductRow> for Product {
    fn from(row: ProductRow) -> Self {
        Self {
            id: row.id,
            seller_id: row.seller_id,
            name: row.name,
            description: row.description,
            price: Money::from_cents(row.price_cents),
            active: row.active,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Clone)]
pub struct SellerRepository {
    pool: PgPool,
}

impl SellerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, seller: &Seller) -> sqlx::Result<()> {
        sqlx::query("INSERT INTO sellers (id, name, email, created_at) VALUES ($1, $2, $3, $4)")
            .bind(seller.id)
            .bind(&seller.name)
            .bind(&seller.email)
            .bind(seller.created_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: Uuid) -> sqlx::Result<Option<Seller>> {
        let row = sqlx::query_as::<_, SellerRow>(
            "SELECT id, name, email, created_at FROM sellers WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Seller::from))
    }

    pub async fn list(&self) -> sqlx::Result<Vec<Seller>> {
        let rows = sqlx::query_as::<_, SellerRow>(
            "SELECT id, name, email, created_at FROM sellers ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Seller::from).collect())
    }
}

#[derive(Clone)]
pub struct ProductRepository {
    pool: PgPool,
}

const PRODUCT_COLUMNS: &str =
    "id, seller_id, name, description, price_cents, active, created_at, updated_at";

impl ProductRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, product: &Product) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO products \
             (id, seller_id, name, description, price_cents, active, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(product.id)
        .bind(product.seller_id)
        .bind(&product.name)
        .bind(&product.description)
        .bind(product.price.cents())
        .bind(product.active)
        .bind(product.created_at)
        .bind(product.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update(&self, product: &Product) -> sqlx::Result<bool> {
        let result = sqlx::query(
            "UPDATE products \
             SET name = $2, description = $3, price_cents = $4, active = $5, updated_at = $6 \
             WHERE id = $1",
        )
        .bind(product.id)
        .bind(&product.name)
        .bind(&product.description)
        .bind(product.price.cents())
        .bind(product.active)
        .bind(product.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn find_by_id(&self, id: Uuid) -> sqlx::Result<Option<Product>> {
        let row = sqlx::query_as::<_, ProductRow>(&format!(
            "SELECT {PRODUCT_COLUMNS} FROM products WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Product::from))
    }

    pub async fn list(&self) -> sqlx::Result<Vec<Product>> {
        let rows = sqlx::query_as::<_, ProductRow>(&format!(
            "SELECT {PRODUCT_COLUMNS} FROM products ORDER BY created_at"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Product::from).collect())
    }

    pub async fn list_by_seller(&self, seller_id: Uuid) -> sqlx::Result<Vec<Product>> {
        let rows = sqlx::query_as::<_, ProductRow>(&format!(
            "SELECT {PRODUCT_COLUMNS} FROM products WHERE seller_id = $1 ORDER BY created_at"
        ))
        .bind(seller_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Product::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seller() -> Seller {
        Seller::new("Loja da Ana", format!("{}@loja.com", Uuid::new_v4())).unwrap()
    }

    #[sqlx::test]
    async fn insere_e_busca_vendedor(pool: PgPool) {
        let repo = SellerRepository::new(pool);
        let seller = seller();

        repo.insert(&seller).await.unwrap();
        let found = repo.find_by_id(seller.id).await.unwrap();

        assert_eq!(found, Some(seller));
    }

    #[sqlx::test]
    async fn vendedor_inexistente_retorna_none(pool: PgPool) {
        let repo = SellerRepository::new(pool);

        assert_eq!(repo.find_by_id(Uuid::new_v4()).await.unwrap(), None);
    }

    #[sqlx::test]
    async fn email_de_vendedor_e_unico(pool: PgPool) {
        let repo = SellerRepository::new(pool);
        let first = seller();
        let duplicate = Seller::new("Outra Loja", first.email.clone()).unwrap();

        repo.insert(&first).await.unwrap();
        let err = repo.insert(&duplicate).await.unwrap_err();

        assert!(
            err.as_database_error()
                .is_some_and(|e| e.is_unique_violation()),
            "esperava violação de unicidade, veio {err}"
        );
    }

    #[sqlx::test]
    async fn insere_atualiza_e_lista_produtos(pool: PgPool) {
        let sellers = SellerRepository::new(pool.clone());
        let products = ProductRepository::new(pool);
        let seller = seller();
        sellers.insert(&seller).await.unwrap();

        let mut product =
            Product::new(seller.id, "Teclado", Some("Mecânico".to_owned()), 25_000).unwrap();
        products.insert(&product).await.unwrap();

        product.update_price(30_000).unwrap();
        product.deactivate();
        assert!(products.update(&product).await.unwrap());

        let found = products.find_by_id(product.id).await.unwrap().unwrap();
        assert_eq!(found.price, Money::from_cents(30_000));
        assert!(!found.active);

        assert_eq!(
            products.list_by_seller(seller.id).await.unwrap(),
            vec![found]
        );
    }

    #[sqlx::test]
    async fn update_de_produto_inexistente_retorna_false(pool: PgPool) {
        let products = ProductRepository::new(pool);
        let orphan = Product::new(Uuid::new_v4(), "Fantasma", None, 100).unwrap();

        assert!(!products.update(&orphan).await.unwrap());
    }

    #[sqlx::test]
    async fn produto_exige_vendedor_existente(pool: PgPool) {
        let products = ProductRepository::new(pool);
        let orphan = Product::new(Uuid::new_v4(), "Sem dono", None, 100).unwrap();

        let err = products.insert(&orphan).await.unwrap_err();

        assert!(err
            .as_database_error()
            .is_some_and(|e| e.is_foreign_key_violation()));
    }
}
