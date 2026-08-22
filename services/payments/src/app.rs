use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use shared::api::Health;
use shared::AppConfig;
use sqlx::PgPool;

use crate::gateway::SimulatedGateway;
use crate::handlers;
use crate::repository::PaymentRepository;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub pool: PgPool,
    pub payments: PaymentRepository,
    pub gateway: SimulatedGateway,
}

impl AppState {
    pub fn new(config: AppConfig, pool: PgPool, gateway: SimulatedGateway) -> Self {
        Self {
            config: Arc::new(config),
            payments: PaymentRepository::new(pool.clone()),
            pool,
            gateway,
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/payments", axum::routing::post(handlers::create))
        .route("/payments/{id}", get(handlers::get))
        .route("/payments/order/{order_id}", get(handlers::get_by_order))
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
