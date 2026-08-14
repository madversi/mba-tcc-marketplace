use catalog::app::{router, AppState};
use shared::{AppConfig, DatabaseConfig};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::from_env(env!("CARGO_PKG_NAME"))?;
    let addr = config.bind_addr();

    let pool = shared::db::connect(&DatabaseConfig::from_env()?).await?;
    sqlx::migrate!().run(&pool).await?;

    let state = AppState::new(config, pool);
    let listener = TcpListener::bind(&addr).await?;
    println!("{} ouvindo em http://{addr}", state.config.service_name);

    axum::serve(listener, router(state)).await?;
    Ok(())
}
