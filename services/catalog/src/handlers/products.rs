use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use domain::Product;
use serde::Deserialize;
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;

#[derive(Deserialize)]
pub struct CreateProduct {
    pub seller_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub price_cents: i64,
}

#[derive(Deserialize)]
pub struct UpdateProduct {
    pub name: Option<String>,
    pub description: Option<String>,
    pub price_cents: Option<i64>,
    pub active: Option<bool>,
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateProduct>,
) -> Result<(StatusCode, Json<Product>), ApiError> {
    let product = Product::new(
        body.seller_id,
        body.name,
        body.description,
        body.price_cents,
    )?;
    match state.products.insert(&product).await {
        Ok(()) => Ok((StatusCode::CREATED, Json(product))),
        Err(err) => match ApiError::from(err) {
            ApiError::Validation(_) => Err(ApiError::Validation("vendedor inexistente".to_owned())),
            other => Err(other),
        },
    }
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Product>>, ApiError> {
    Ok(Json(state.products.list().await?))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Product>, ApiError> {
    find(&state, id).await.map(Json)
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateProduct>,
) -> Result<Json<Product>, ApiError> {
    let mut product = find(&state, id).await?;

    if let Some(name) = body.name {
        product.rename(name)?;
    }
    if let Some(description) = body.description {
        product.set_description(Some(description));
    }
    if let Some(price_cents) = body.price_cents {
        product.update_price(price_cents)?;
    }
    match body.active {
        Some(true) => product.activate(),
        Some(false) => product.deactivate(),
        None => {}
    }

    state.products.update(&product).await?;
    Ok(Json(product))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut product = find(&state, id).await?;
    product.deactivate();
    state.products.update(&product).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn find(state: &AppState, id: Uuid) -> Result<Product, ApiError> {
    state
        .products
        .find_by_id(id)
        .await?
        .ok_or(ApiError::NotFound("produto"))
}
