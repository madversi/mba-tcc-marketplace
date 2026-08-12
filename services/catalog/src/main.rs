use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use shared::AppConfig;
use tokio::net::TcpListener;

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: String,
    version: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::from_env(env!("CARGO_PKG_NAME"))?;
    let addr = config.bind_addr();

    let state = AppState {
        config: Arc::new(config),
    };

    let listener = TcpListener::bind(&addr).await?;
    println!("{} ouvindo em http://{addr}", state.config.service_name);

    axum::serve(listener, router(state)).await?;
    Ok(())
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        service: state.config.service_name.clone(),
        version: env!("CARGO_PKG_VERSION"),
    })
}
