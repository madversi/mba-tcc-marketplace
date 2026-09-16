use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, post};
use axum::{Json, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use shared::api::Health;
use shared::metrics::track_http;
use shared::{AppConfig, EventBus};
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

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

pub fn router(state: AppState, metrics: PrometheusHandle) -> Router {
    let timeout = state.config.request_timeout;
    Router::new()
        .route("/health", get(health))
        .route("/orders", post(handlers::create))
        .route("/orders/{id}", get(handlers::get))
        .route_layer(shared::api::request_timeout(timeout))
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
