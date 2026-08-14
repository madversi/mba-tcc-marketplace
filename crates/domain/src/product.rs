use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;
use crate::money::Money;
use crate::seller::non_empty;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Product {
    pub id: Uuid,
    pub seller_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub price: Money,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Product {
    pub fn new(
        seller_id: Uuid,
        name: impl Into<String>,
        description: Option<String>,
        price_cents: i64,
    ) -> Result<Self, DomainError> {
        let now = crate::time::now();

        Ok(Self {
            id: Uuid::new_v4(),
            seller_id,
            name: non_empty(name.into(), "nome do produto")?,
            description: description
                .map(|d| d.trim().to_owned())
                .filter(|d| !d.is_empty()),
            price: Money::positive(price_cents)?,
            active: true,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update_price(&mut self, price_cents: i64) -> Result<(), DomainError> {
        self.price = Money::positive(price_cents)?;
        self.updated_at = crate::time::now();
        Ok(())
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.updated_at = crate::time::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cria_produto_ativo_com_preco_valido() {
        let product = Product::new(Uuid::new_v4(), "Teclado", None, 15_000).unwrap();

        assert!(product.active);
        assert_eq!(product.price, Money::from_cents(15_000));
        assert_eq!(product.description, None);
    }

    #[test]
    fn descricao_em_branco_vira_none() {
        let product =
            Product::new(Uuid::new_v4(), "Teclado", Some("   ".to_owned()), 15_000).unwrap();

        assert_eq!(product.description, None);
    }

    #[test]
    fn rejeita_preco_nao_positivo() {
        assert_eq!(
            Product::new(Uuid::new_v4(), "Teclado", None, 0).unwrap_err(),
            DomainError::NonPositiveAmount(0)
        );
    }

    #[test]
    fn rejeita_nome_vazio() {
        assert_eq!(
            Product::new(Uuid::new_v4(), "", None, 100).unwrap_err(),
            DomainError::EmptyField {
                field: "nome do produto"
            }
        );
    }

    #[test]
    fn atualiza_preco_e_desativa() {
        let mut product = Product::new(Uuid::new_v4(), "Teclado", None, 100).unwrap();

        product.update_price(200).unwrap();
        assert_eq!(product.price, Money::from_cents(200));
        assert!(product.update_price(-1).is_err());

        product.deactivate();
        assert!(!product.active);
    }
}
