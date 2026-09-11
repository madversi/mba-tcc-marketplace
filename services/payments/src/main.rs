use payments::app::{router, AppState};
use payments::consumer;
use payments::gateway::SimulatedGateway;
use shared::{AmqpConfig, AppConfig, DatabaseConfig, EventBus};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AppConfig::from_env(env!("CARGO_PKG_NAME"))?;
    shared::telemetry::init_from_app_config(&config);
    let metrics = shared::metrics::init();
    let addr = config.bind_addr();

    let pool = shared::db::connect(&DatabaseConfig::from_env()?).await?;
    sqlx::migrate!().run(&pool).await?;
    let bus = EventBus::connect(&AmqpConfig::from_env()?).await?;

    let state = AppState::new(config, pool, SimulatedGateway);
    let _consumer = consumer::spawn(
        bus,
        state.payments.clone(),
        state.gateway.clone(),
        "payments",
    )
    .await?;

    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(%addr, service = %state.config.service_name, "ouvindo");

    axum::serve(listener, router(state, metrics)).await?;
    Ok(())
}
