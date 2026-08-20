use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use domain::{Product, Seller};
use serde::Deserialize;
use uuid::Uuid;

use shared::ApiError;

use crate::app::AppState;

#[derive(Deserialize)]
pub struct CreateSeller {
    pub name: String,
    pub email: String,
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateSeller>,
) -> Result<(StatusCode, Json<Seller>), ApiError> {
    let seller = Seller::new(body.name, body.email)?;
    match state.sellers.insert(&seller).await {
        Ok(()) => Ok((StatusCode::CREATED, Json(seller))),
        Err(err) => match ApiError::from(err) {
            ApiError::Conflict(_) => Err(ApiError::Conflict("email já cadastrado".to_owned())),
            other => Err(other),
        },
    }
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Seller>>, ApiError> {
    Ok(Json(state.sellers.list().await?))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Seller>, ApiError> {
    state
        .sellers
        .find_by_id(id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound("vendedor"))
}

pub async fn list_products(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Product>>, ApiError> {
    if state.sellers.find_by_id(id).await?.is_none() {
        return Err(ApiError::NotFound("vendedor"));
    }
    Ok(Json(state.products.list_by_seller(id).await?))
}
