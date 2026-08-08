use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("{field} não pode ser vazio")]
    EmptyField { field: &'static str },

    #[error("valor monetário deve ser positivo, recebido {0} centavos")]
    NonPositiveAmount(i64),

    #[error("overflow em operação monetária")]
    MoneyOverflow,

    #[error("quantidade deve ser maior que zero")]
    ZeroQuantity,

    #[error("pedido deve conter ao menos um item")]
    EmptyOrder,

    #[error(
        "estoque insuficiente para o produto {product_id}: disponível {available}, solicitado {requested}"
    )]
    InsufficientStock {
        product_id: Uuid,
        available: u32,
        requested: u32,
    },

    #[error(
        "produto {product_id} tem apenas {reserved} unidades reservadas, tentativa de liberar {requested}"
    )]
    ReleaseExceedsReserved {
        product_id: Uuid,
        reserved: u32,
        requested: u32,
    },

    #[error("transição de status inválida: {from} -> {to}")]
    InvalidTransition {
        from: &'static str,
        to: &'static str,
    },
}
