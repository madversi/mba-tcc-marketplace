use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use shared::api::Health;
use shared::{AppConfig, EventBus};
use sqlx::PgPool;

use crate::catalog_client::CatalogClient;
use crate::handlers;
use crate::repository::OrderRepository;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub pool: PgPool,
    pub orders: OrderRepository,
    pub catalog: CatalogClient,
    pub bus: EventBus,
}

impl AppState {
    pub fn new(config: AppConfig, pool: PgPool, catalog: CatalogClient, bus: EventBus) -> Self {
        Self {
            config: Arc::new(config),
            orders: OrderRepository::new(pool.clone()),
            pool,
            catalog,
            bus,
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/orders", post(handlers::create))
        .route("/orders/{id}", get(handlers::get))
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
