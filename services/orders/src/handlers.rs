use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use domain::events::OrderCreated;
use domain::{Money, Order, OrderItem};
use serde::Deserialize;
use shared::ApiError;
use uuid::Uuid;

use crate::app::AppState;
use crate::catalog_client::CatalogError;

impl From<CatalogError> for ApiError {
    fn from(err: CatalogError) -> Self {
        Self::Unavailable(err.to_string())
    }
}

#[derive(Deserialize)]
pub struct CreateOrder {
    pub buyer_id: Uuid,
    pub items: Vec<CreateItem>,
}

#[derive(Deserialize)]
pub struct CreateItem {
    pub product_id: Uuid,
    pub quantity: u32,
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateOrder>,
) -> Result<(StatusCode, Json<Order>), ApiError> {
    let mut items = Vec::with_capacity(body.items.len());
    for item in body.items {
        let product = state
            .catalog
            .get_product(item.product_id)
            .await?
            .ok_or_else(|| {
                ApiError::Validation(format!(
                    "produto {} não encontrado no catálogo",
                    item.product_id
                ))
            })?;
        if !product.active {
            return Err(ApiError::Validation(format!(
                "produto {} está inativo",
                product.id
            )));
        }
        items.push(OrderItem::new(
            product.id,
            item.quantity,
            Money::from_cents(product.price),
        )?);
    }

    let order = Order::new(body.buyer_id, items)?;
    state.orders.insert(&order).await?;

    let event = OrderCreated {
        order_id: order.id,
        buyer_id: order.buyer_id,
        items: order.items.clone(),
        total: order.total,
        occurred_at: domain::time::now(),
    };
    if let Err(err) = state.bus.publish(&event).await {
        eprintln!(
            "falha ao publicar order.created do pedido {}: {err}",
            order.id
        );
    }

    Ok((StatusCode::CREATED, Json(order)))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Order>, ApiError> {
    state
        .orders
        .find_by_id(id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound("pedido"))
}
