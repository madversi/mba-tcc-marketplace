use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StockItem {
    pub product_id: Uuid,
    pub available: u32,
    pub reserved: u32,
    pub updated_at: DateTime<Utc>,
}

impl StockItem {
    pub fn new(product_id: Uuid, available: u32) -> Self {
        Self {
            product_id,
            available,
            reserved: 0,
            updated_at: crate::time::now(),
        }
    }

    pub fn add(&mut self, quantity: u32) -> Result<(), DomainError> {
        ensure_positive(quantity)?;
        self.available = self.available.saturating_add(quantity);
        self.touch();
        Ok(())
    }

    pub fn reserve(&mut self, quantity: u32) -> Result<(), DomainError> {
        ensure_positive(quantity)?;
        if quantity > self.available {
            return Err(DomainError::InsufficientStock {
                product_id: self.product_id,
                available: self.available,
                requested: quantity,
            });
        }
        self.available -= quantity;
        self.reserved += quantity;
        self.touch();
        Ok(())
    }

    pub fn release(&mut self, quantity: u32) -> Result<(), DomainError> {
        ensure_positive(quantity)?;
        self.take_reserved(quantity)?;
        self.available += quantity;
        self.touch();
        Ok(())
    }

    pub fn commit(&mut self, quantity: u32) -> Result<(), DomainError> {
        ensure_positive(quantity)?;
        self.take_reserved(quantity)?;
        self.touch();
        Ok(())
    }

    fn take_reserved(&mut self, quantity: u32) -> Result<(), DomainError> {
        if quantity > self.reserved {
            return Err(DomainError::ReleaseExceedsReserved {
                product_id: self.product_id,
                reserved: self.reserved,
                requested: quantity,
            });
        }
        self.reserved -= quantity;
        Ok(())
    }

    fn touch(&mut self) {
        self.updated_at = crate::time::now();
    }
}

fn ensure_positive(quantity: u32) -> Result<(), DomainError> {
    if quantity == 0 {
        return Err(DomainError::ZeroQuantity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock(available: u32) -> StockItem {
        StockItem::new(Uuid::new_v4(), available)
    }

    #[test]
    fn reserva_move_de_disponivel_para_reservado() {
        let mut item = stock(10);

        item.reserve(3).unwrap();

        assert_eq!((item.available, item.reserved), (7, 3));
    }

    #[test]
    fn reserva_alem_do_disponivel_falha_sem_alterar_estado() {
        let mut item = stock(2);

        let err = item.reserve(5).unwrap_err();

        assert!(matches!(
            err,
            DomainError::InsufficientStock {
                available: 2,
                requested: 5,
                ..
            }
        ));
        assert_eq!((item.available, item.reserved), (2, 0));
    }

    #[test]
    fn liberacao_devolve_ao_disponivel() {
        let mut item = stock(10);
        item.reserve(4).unwrap();

        item.release(4).unwrap();

        assert_eq!((item.available, item.reserved), (10, 0));
    }

    #[test]
    fn commit_consome_a_reserva() {
        let mut item = stock(10);
        item.reserve(4).unwrap();

        item.commit(4).unwrap();

        assert_eq!((item.available, item.reserved), (6, 0));
    }

    #[test]
    fn liberar_ou_consumir_mais_que_o_reservado_falha() {
        let mut item = stock(10);
        item.reserve(1).unwrap();

        assert!(matches!(
            item.release(2).unwrap_err(),
            DomainError::ReleaseExceedsReserved {
                reserved: 1,
                requested: 2,
                ..
            }
        ));
        assert!(item.commit(2).is_err());
    }

    #[test]
    fn quantidade_zero_e_rejeitada_em_todas_as_operacoes() {
        let mut item = stock(10);

        assert_eq!(item.add(0), Err(DomainError::ZeroQuantity));
        assert_eq!(item.reserve(0), Err(DomainError::ZeroQuantity));
        assert_eq!(item.release(0), Err(DomainError::ZeroQuantity));
        assert_eq!(item.commit(0), Err(DomainError::ZeroQuantity));
    }
}
