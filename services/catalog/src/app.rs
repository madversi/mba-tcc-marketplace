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

use crate::handlers::{products, sellers};
use crate::repository::{ProductRepository, SellerRepository};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub pool: PgPool,
    pub sellers: SellerRepository,
    pub products: ProductRepository,
}

impl AppState {
    pub fn new(config: AppConfig, pool: PgPool) -> Self {
        Self {
            config: Arc::new(config),
            sellers: SellerRepository::new(pool.clone()),
            products: ProductRepository::new(pool.clone()),
            pool,
        }
    }
}

pub fn router(state: AppState, metrics: PrometheusHandle) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sellers", get(sellers::list).post(sellers::create))
        .route("/sellers/{id}", get(sellers::get))
        .route("/sellers/{id}/products", get(sellers::list_products))
        .route("/products", get(products::list).post(products::create))
        .route(
            "/products/{id}",
            get(products::get)
                .patch(products::update)
                .delete(products::delete),
        )
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
