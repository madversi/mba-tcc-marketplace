use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use shared::AppConfig;
use sqlx::PgPool;

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

pub fn router(state: AppState) -> Router {
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
        .with_state(state)
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: String,
    version: &'static str,
    database: &'static str,
}

async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();

    let (status_code, status, database) = if db_ok {
        (StatusCode::OK, "ok", "up")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "degraded", "down")
    };

    (
        status_code,
        Json(Health {
            status,
            service: state.config.service_name.clone(),
            version: env!("CARGO_PKG_VERSION"),
            database,
        }),
    )
}
