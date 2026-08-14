use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::DomainError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seller {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

impl Seller {
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Result<Self, DomainError> {
        let name = non_empty(name.into(), "nome do vendedor")?;
        let email = non_empty(email.into(), "email do vendedor")?;

        Ok(Self {
            id: Uuid::new_v4(),
            name,
            email,
            created_at: crate::time::now(),
        })
    }
}

pub(crate) fn non_empty(value: String, field: &'static str) -> Result<String, DomainError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(DomainError::EmptyField { field });
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cria_vendedor_normalizando_espacos() {
        let seller = Seller::new("  Loja da Ana ", "ana@loja.com").unwrap();

        assert_eq!(seller.name, "Loja da Ana");
        assert_eq!(seller.email, "ana@loja.com");
    }

    #[test]
    fn rejeita_nome_ou_email_vazios() {
        assert_eq!(
            Seller::new("   ", "a@b.com").unwrap_err(),
            DomainError::EmptyField {
                field: "nome do vendedor"
            }
        );
        assert_eq!(
            Seller::new("Loja", "").unwrap_err(),
            DomainError::EmptyField {
                field: "email do vendedor"
            }
        );
    }
}
