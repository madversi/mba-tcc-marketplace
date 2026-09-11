use std::error::Error;
use std::future::Future;

use domain::events::Event;
use futures_util::StreamExt;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions, BasicQosOptions,
    ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions,
};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind};
use tokio::task::JoinHandle;
use tracing::Instrument;

use crate::config::AmqpConfig;

pub type HandlerError = Box<dyn Error + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum AmqpError {
    #[error("falha na comunicação com o broker: {0}")]
    Broker(#[from] lapin::Error),

    #[error("falha ao serializar evento: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Clone)]
pub struct EventBus {
    channel: Channel,
    exchange: String,
}

impl EventBus {
    pub async fn connect(config: &AmqpConfig) -> Result<Self, AmqpError> {
        let connection = Connection::connect(&config.url, ConnectionProperties::default()).await?;
        let channel = connection.create_channel().await?;
        channel
            .basic_qos(config.prefetch, BasicQosOptions::default())
            .await?;
        channel
            .exchange_declare(
                &config.exchange,
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        Ok(Self {
            channel,
            exchange: config.exchange.clone(),
        })
    }

    pub async fn publish<E: Event>(&self, event: &E) -> Result<(), AmqpError> {
        let payload = serde_json::to_vec(event)?;
        self.channel
            .basic_publish(
                &self.exchange,
                E::ROUTING_KEY,
                BasicPublishOptions::default(),
                &payload,
                BasicProperties::default()
                    .with_content_type("application/json".into())
                    .with_delivery_mode(2),
            )
            .await?
            .await?;
        Ok(())
    }

    pub async fn spawn_consumer<E, F, Fut>(
        &self,
        service: &str,
        handler: F,
    ) -> Result<JoinHandle<()>, AmqpError>
    where
        E: Event,
        F: Fn(E) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), HandlerError>> + Send,
    {
        let queue = format!("{service}.{}", E::ROUTING_KEY);
        self.channel
            .queue_declare(
                &queue,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;
        self.channel
            .queue_bind(
                &queue,
                &self.exchange,
                E::ROUTING_KEY,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let mut consumer = self
            .channel
            .basic_consume(
                &queue,
                &format!("{queue}-consumer"),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let handle = tokio::spawn(async move {
            while let Some(delivery) = consumer.next().await {
                let delivery = match delivery {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        tracing::error!(%queue, %err, "consumo interrompido");
                        break;
                    }
                };

                let span = tracing::info_span!("message_processing", %queue);
                async {
                    let start = std::time::Instant::now();
                    let outcome = match serde_json::from_slice::<E>(&delivery.data) {
                        Ok(event) => handler(event).await,
                        Err(err) => Err(HandlerError::from(err)),
                    };
                    let elapsed = start.elapsed().as_secs_f64();

                    let status = if outcome.is_ok() { "ok" } else { "error" };
                    metrics::histogram!(
                        "message_processing_duration_seconds",
                        "queue" => queue.clone(),
                        "status" => status,
                    )
                    .record(elapsed);

                    let result = match outcome {
                        Ok(()) => delivery.ack(BasicAckOptions::default()).await,
                        Err(err) => {
                            tracing::error!(%queue, %err, "handler falhou");
                            delivery
                                .nack(BasicNackOptions {
                                    requeue: false,
                                    ..Default::default()
                                })
                                .await
                        }
                    };
                    if let Err(err) = result {
                        tracing::error!(%queue, %err, "falha ao confirmar mensagem");
                    }
                }
                .instrument(span)
                .await;
            }
        });

        Ok(handle)
    }
}
