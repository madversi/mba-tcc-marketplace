use inventory::app::{router, AppState};
use inventory::consumer;
use shared::{AmqpConfig, AppConfig, DatabaseConfig, EventBus};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::from_env(env!("CARGO_PKG_NAME"))?;
    let addr = config.bind_addr();

    let pool = shared::db::connect(&DatabaseConfig::from_env()?).await?;
    sqlx::migrate!().run(&pool).await?;
    let bus = EventBus::connect(&AmqpConfig::from_env()?).await?;

    let state = AppState::new(config, pool);
    let _consumer = consumer::spawn(bus, state.stock.clone(), "inventory").await?;

    let listener = TcpListener::bind(&addr).await?;
    println!("{} ouvindo em http://{addr}", state.config.service_name);

    axum::serve(listener, router(state)).await?;
    Ok(())
}
