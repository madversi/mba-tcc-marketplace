use std::thread;

use uuid::Uuid;

use crate::amqp::EventBus;
use crate::config::AmqpConfig;

pub struct IsolatedBroker {
    config: AmqpConfig,
    bus: EventBus,
}

impl IsolatedBroker {
    pub async fn connect() -> Self {
        Self::connect_with(|_| {}).await
    }

    pub async fn connect_with(configure: impl FnOnce(&mut AmqpConfig)) -> Self {
        let mut config = AmqpConfig::from_env().expect("AMQP_URL ausente (suba o docker-compose)");
        config.exchange = format!("test-{}", Uuid::new_v4());
        configure(&mut config);
        let bus = EventBus::connect(&config)
            .await
            .expect("broker inacessível");
        Self { config, bus }
    }

    pub fn bus(&self) -> EventBus {
        self.bus.clone()
    }

    pub fn config(&self) -> &AmqpConfig {
        &self.config
    }
}

impl Drop for IsolatedBroker {
    fn drop(&mut self) {
        let config = self.config.clone();
        let _ = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(delete_exchanges(&config))
        })
        .join();
    }
}

async fn delete_exchanges(config: &AmqpConfig) -> Result<(), lapin::Error> {
    let connection =
        lapin::Connection::connect(&config.url, lapin::ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;
    for suffix in ["", ".retry", ".requeue", ".dead"] {
        channel
            .exchange_delete(
                &format!("{}{suffix}", config.exchange),
                lapin::options::ExchangeDeleteOptions::default(),
            )
            .await?;
    }
    Ok(())
}
