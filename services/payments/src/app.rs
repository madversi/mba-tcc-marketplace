use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::get;
use axum::{Json, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use shared::api::Health;
use shared::metrics::track_http;
use shared::AppConfig;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

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

pub fn router(state: AppState, metrics: PrometheusHandle) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/payments", axum::routing::post(handlers::create))
        .route("/payments/{id}", get(handlers::get))
        .route("/payments/order/{order_id}", get(handlers::get_by_order))
        .route_layer(middleware::from_fn(track_http))
        .route("/metrics", get(move || shared::metrics::render(metrics)))
        .layer(TraceLayer::new_for_http())
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
