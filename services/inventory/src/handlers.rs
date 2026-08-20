use axum::extract::{Path, State};
use axum::Json;
use domain::StockItem;
use serde::Deserialize;
use shared::ApiError;
use uuid::Uuid;

use crate::app::AppState;
use crate::repository::StockError;

impl From<StockError> for ApiError {
    fn from(err: StockError) -> Self {
        match err {
            StockError::NotFound => Self::NotFound("estoque do produto"),
            StockError::Domain(e) => e.into(),
            StockError::Db(e) => e.into(),
        }
    }
}

#[derive(Deserialize)]
pub struct Quantity {
    pub quantity: u32,
}

pub async fn get(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
) -> Result<Json<StockItem>, ApiError> {
    state
        .stock
        .find(product_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound("estoque do produto"))
}

pub async fn set_available(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
    Json(body): Json<Quantity>,
) -> Result<Json<StockItem>, ApiError> {
    Ok(Json(
        state.stock.set_available(product_id, body.quantity).await?,
    ))
}

pub async fn reserve(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
    Json(body): Json<Quantity>,
) -> Result<Json<StockItem>, ApiError> {
    let item = state
        .stock
        .modify(product_id, |s| s.reserve(body.quantity))
        .await?;
    Ok(Json(item))
}

pub async fn release(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
    Json(body): Json<Quantity>,
) -> Result<Json<StockItem>, ApiError> {
    let item = state
        .stock
        .modify(product_id, |s| s.release(body.quantity))
        .await?;
    Ok(Json(item))
}

pub async fn commit(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
    Json(body): Json<Quantity>,
) -> Result<Json<StockItem>, ApiError> {
    let item = state
        .stock
        .modify(product_id, |s| s.commit(body.quantity))
        .await?;
    Ok(Json(item))
}
