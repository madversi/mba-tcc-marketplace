use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use shared::api::Health;
use shared::AppConfig;
use sqlx::PgPool;

use crate::handlers;
use crate::repository::StockRepository;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub pool: PgPool,
    pub stock: StockRepository,
}

impl AppState {
    pub fn new(config: AppConfig, pool: PgPool) -> Self {
        Self {
            config: Arc::new(config),
            stock: StockRepository::new(pool.clone()),
            pool,
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route(
            "/stock/{product_id}",
            get(handlers::get).put(handlers::set_available),
        )
        .route("/stock/{product_id}/reserve", post(handlers::reserve))
        .route("/stock/{product_id}/release", post(handlers::release))
        .route("/stock/{product_id}/commit", post(handlers::commit))
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    shared::api::health(
        &state.pool,
        &state.config.service_name,
        env!("CARGO_PKG_VERSION"),
    )
    .await
}
