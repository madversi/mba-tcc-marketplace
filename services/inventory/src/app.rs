use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, post};
use axum::{Json, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use shared::api::Health;
use shared::metrics::track_http;
use shared::AppConfig;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

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

pub fn router(state: AppState, metrics: PrometheusHandle) -> Router {
    Router::new()
        .route("/health", get(health))
        .route(
            "/stock/{product_id}",
            get(handlers::get).put(handlers::set_available),
        )
        .route("/stock/{product_id}/reserve", post(handlers::reserve))
        .route("/stock/{product_id}/release", post(handlers::release))
        .route("/stock/{product_id}/commit", post(handlers::commit))
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
