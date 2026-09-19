use std::error::Error;
use std::future::Future;

use domain::events::Event;
use futures_util::{future, StreamExt};
use lapin::message::Delivery;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions, BasicQosOptions,
    ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions,
};
use lapin::types::{AMQPValue, FieldTable};
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
    retry_ttl_ms: u64,
    max_attempts: u32,
    consumer_concurrency: usize,
}

impl EventBus {
    pub async fn connect(config: &AmqpConfig) -> Result<Self, AmqpError> {
        let connection = Connection::connect(&config.url, ConnectionProperties::default()).await?;
        let channel = connection.create_channel().await?;
        let prefetch = config
            .prefetch
            .max(u16::try_from(config.consumer_concurrency).unwrap_or(u16::MAX));
        channel
            .basic_qos(prefetch, BasicQosOptions::default())
            .await?;

        let bus = Self {
            channel,
            exchange: config.exchange.clone(),
            retry_ttl_ms: config.retry_ttl_ms,
            max_attempts: config.max_attempts,
            consumer_concurrency: config.consumer_concurrency,
        };

        bus.declare_exchange(&bus.exchange, ExchangeKind::Topic)
            .await?;
        bus.declare_exchange(&bus.retry_exchange(), ExchangeKind::Direct)
            .await?;
        bus.declare_exchange(&bus.requeue_exchange(), ExchangeKind::Direct)
            .await?;
        bus.declare_exchange(&bus.dead_exchange(), ExchangeKind::Direct)
            .await?;

        Ok(bus)
    }

    pub fn with_retry(&self, retry_ttl_ms: u64, max_attempts: u32) -> Self {
        Self {
            retry_ttl_ms,
            max_attempts,
            ..self.clone()
        }
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
        self.declare_retry_topology(&queue).await?;
        self.channel
            .queue_bind(
                &queue,
                &self.exchange,
                E::ROUTING_KEY,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let consumer = self
            .channel
            .basic_consume(
                &queue,
                &format!("{queue}-consumer"),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let channel = self.channel.clone();
        let dead_exchange = self.dead_exchange();
        let max_attempts = self.max_attempts;
        let concurrency = self.consumer_concurrency;

        let handle = tokio::spawn(async move {
            let queue = queue.as_str();
            let handler = &handler;
            let channel = &channel;
            let dead_exchange = dead_exchange.as_str();

            consumer
                .scan((), |_, delivery| {
                    future::ready(match delivery {
                        Ok(delivery) => Some(delivery),
                        Err(err) => {
                            tracing::error!(%queue, %err, "consumo interrompido");
                            None
                        }
                    })
                })
                .for_each_concurrent(concurrency, |delivery| {
                    process_delivery(
                        handler,
                        channel,
                        dead_exchange,
                        queue,
                        max_attempts,
                        delivery,
                    )
                })
                .await;
        });

        Ok(handle)
    }

    fn retry_exchange(&self) -> String {
        format!("{}.retry", self.exchange)
    }

    fn requeue_exchange(&self) -> String {
        format!("{}.requeue", self.exchange)
    }

    fn dead_exchange(&self) -> String {
        format!("{}.dead", self.exchange)
    }

    async fn declare_exchange(&self, name: &str, kind: ExchangeKind) -> Result<(), AmqpError> {
        self.channel
            .exchange_declare(
                name,
                kind,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;
        Ok(())
    }

    async fn declare_retry_topology(&self, queue: &str) -> Result<(), AmqpError> {
        let durable = QueueDeclareOptions {
            durable: true,
            ..Default::default()
        };

        let mut main_args = FieldTable::default();
        main_args.insert(
            "x-dead-letter-exchange".into(),
            AMQPValue::LongString(self.retry_exchange().into()),
        );
        main_args.insert(
            "x-dead-letter-routing-key".into(),
            AMQPValue::LongString(queue.into()),
        );
        self.channel
            .queue_declare(queue, durable, main_args)
            .await?;
        self.channel
            .queue_bind(
                queue,
                &self.requeue_exchange(),
                queue,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let retry_queue = format!("{queue}.retry");
        let mut retry_args = FieldTable::default();
        retry_args.insert(
            "x-message-ttl".into(),
            AMQPValue::LongLongInt(self.retry_ttl_ms as i64),
        );
        retry_args.insert(
            "x-dead-letter-exchange".into(),
            AMQPValue::LongString(self.requeue_exchange().into()),
        );
        retry_args.insert(
            "x-dead-letter-routing-key".into(),
            AMQPValue::LongString(queue.into()),
        );
        self.channel
            .queue_declare(&retry_queue, durable, retry_args)
            .await?;
        self.channel
            .queue_bind(
                &retry_queue,
                &self.retry_exchange(),
                queue,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let dead_queue = format!("{queue}.dead");
        self.channel
            .queue_declare(&dead_queue, durable, FieldTable::default())
            .await?;
        self.channel
            .queue_bind(
                &dead_queue,
                &self.dead_exchange(),
                queue,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        Ok(())
    }
}

async fn process_delivery<E, F, Fut>(
    handler: &F,
    channel: &Channel,
    dead_exchange: &str,
    queue: &str,
    max_attempts: u32,
    delivery: Delivery,
) where
    E: Event,
    F: Fn(E) -> Fut,
    Fut: Future<Output = Result<(), HandlerError>>,
{
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
            "queue" => queue.to_owned(),
            "status" => status,
        )
        .record(elapsed);

        let result = match outcome {
            Ok(()) => delivery.ack(BasicAckOptions::default()).await,
            Err(err) => {
                let attempt = failed_attempts(&delivery.properties, queue) + 1;
                if attempt >= max_attempts {
                    metrics::counter!("message_dead_total", "queue" => queue.to_owned())
                        .increment(1);
                    tracing::error!(
                        %queue, %err, attempt,
                        "tentativas esgotadas; enviando para a fila dead"
                    );
                    send_to_dead(channel, dead_exchange, queue, &delivery).await;
                    delivery.ack(BasicAckOptions::default()).await
                } else {
                    metrics::counter!("message_retry_total", "queue" => queue.to_owned())
                        .increment(1);
                    tracing::warn!(
                        %queue, %err, attempt,
                        "handler falhou; mensagem vai para a fila retry"
                    );
                    delivery
                        .nack(BasicNackOptions {
                            requeue: false,
                            ..Default::default()
                        })
                        .await
                }
            }
        };
        if let Err(err) = result {
            tracing::error!(%queue, %err, "falha ao confirmar mensagem");
        }
    }
    .instrument(span)
    .await;
}

async fn send_to_dead(channel: &Channel, dead_exchange: &str, queue: &str, delivery: &Delivery) {
    let published = channel
        .basic_publish(
            dead_exchange,
            queue,
            BasicPublishOptions::default(),
            &delivery.data,
            delivery.properties.clone(),
        )
        .await;

    match published {
        Ok(confirm) => {
            if let Err(err) = confirm.await {
                tracing::error!(%queue, %err, "falha ao confirmar envio para a fila dead");
            }
        }
        Err(err) => tracing::error!(%queue, %err, "falha ao enviar para a fila dead"),
    }
}

fn failed_attempts(properties: &BasicProperties, queue: &str) -> u32 {
    let Some(headers) = properties.headers() else {
        return 0;
    };
    let Some(AMQPValue::FieldArray(deaths)) = headers.inner().get("x-death") else {
        return 0;
    };

    deaths
        .as_slice()
        .iter()
        .filter_map(|death| match death {
            AMQPValue::FieldTable(table) => Some(table),
            _ => None,
        })
        .find(|table| field_str(table, "queue").as_deref() == Some(queue))
        .and_then(|table| match table.inner().get("count") {
            Some(AMQPValue::LongLongInt(count)) => Some(*count as u32),
            Some(AMQPValue::LongInt(count)) => Some(*count as u32),
            _ => None,
        })
        .unwrap_or(0)
}

fn field_str(table: &FieldTable, key: &str) -> Option<String> {
    match table.inner().get(key) {
        Some(AMQPValue::LongString(value)) => Some(value.to_string()),
        Some(AMQPValue::ShortString(value)) => Some(value.to_string()),
        _ => None,
    }
}
