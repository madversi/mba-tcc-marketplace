use std::time::Duration;

use domain::events::Event;
use serde::{Deserialize, Serialize};
use shared::{AmqpConfig, EventBus};
use uuid::Uuid;

macro_rules! test_event {
    ($name:ident, $key:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        struct $name {
            id: Uuid,
            message: String,
        }

        impl Event for $name {
            const ROUTING_KEY: &'static str = $key;
        }
    };
}

test_event!(RoundTripEvent, "test.shared.roundtrip");
test_event!(FailingEvent, "test.shared.failing");

async fn delete_queue(config: &AmqpConfig, service: &str, routing_key: &str) {
    let connection =
        lapin::Connection::connect(&config.url, lapin::ConnectionProperties::default())
            .await
            .unwrap();
    let channel = connection.create_channel().await.unwrap();
    channel
        .queue_delete(
            &format!("{service}.{routing_key}"),
            lapin::options::QueueDeleteOptions::default(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn publica_e_consome_um_evento() {
    let config = AmqpConfig::from_env().expect("AMQP_URL ausente (suba o docker-compose)");
    let bus = EventBus::connect(&config)
        .await
        .expect("broker inacessível");

    let service = format!("test-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RoundTripEvent>(1);

    let consumer = bus
        .spawn_consumer::<RoundTripEvent, _, _>(&service, move |event| {
            let tx = tx.clone();
            async move {
                tx.send(event).await.unwrap();
                Ok(())
            }
        })
        .await
        .unwrap();

    let sent = RoundTripEvent {
        id: Uuid::new_v4(),
        message: "olá, saga".to_owned(),
    };
    bus.publish(&sent).await.unwrap();

    let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("evento não chegou em 5s")
        .unwrap();
    assert_eq!(received, sent);

    consumer.abort();
    delete_queue(&config, &service, RoundTripEvent::ROUTING_KEY).await;
}

#[tokio::test]
async fn handler_com_erro_descarta_sem_requeue() {
    let config = AmqpConfig::from_env().expect("AMQP_URL ausente (suba o docker-compose)");
    let bus = EventBus::connect(&config)
        .await
        .expect("broker inacessível");

    let service = format!("test-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Uuid>(4);

    let consumer = bus
        .spawn_consumer::<FailingEvent, _, _>(&service, move |event: FailingEvent| {
            let tx = tx.clone();
            async move {
                tx.send(event.id).await.unwrap();
                Err("falha proposital".into())
            }
        })
        .await
        .unwrap();

    let event = FailingEvent {
        id: Uuid::new_v4(),
        message: "vou falhar".to_owned(),
    };
    bus.publish(&event).await.unwrap();

    let first = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("evento não chegou")
        .unwrap();
    assert_eq!(first, event.id);

    let redelivery = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
    assert!(redelivery.is_err(), "mensagem foi reentregue indevidamente");

    consumer.abort();
    delete_queue(&config, &service, FailingEvent::ROUTING_KEY).await;
}
