use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
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
test_event!(RetryEvent, "test.shared.retry");
test_event!(DeadEvent, "test.shared.dead");

async fn channel(config: &AmqpConfig) -> lapin::Channel {
    lapin::Connection::connect(&config.url, lapin::ConnectionProperties::default())
        .await
        .unwrap()
        .create_channel()
        .await
        .unwrap()
}

async fn delete_queue_family(config: &AmqpConfig, service: &str, routing_key: &str) {
    let channel = channel(config).await;
    let queue = format!("{service}.{routing_key}");
    for name in [
        queue.clone(),
        format!("{queue}.retry"),
        format!("{queue}.dead"),
    ] {
        channel
            .queue_delete(&name, lapin::options::QueueDeleteOptions::default())
            .await
            .unwrap();
    }
}

fn has_series(metrics: &str, name: &str, labels: &[&str]) -> bool {
    metrics
        .lines()
        .any(|line| line.starts_with(name) && labels.iter().all(|label| line.contains(label)))
}

async fn recv(rx: &mut tokio::sync::mpsc::Receiver<Uuid>) -> Uuid {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("evento não chegou em 5s")
        .unwrap()
}

async fn wait_for_dead_message(config: &AmqpConfig, queue: &str) -> Option<Vec<u8>> {
    let channel = channel(config).await;
    for _ in 0..50 {
        let message = channel
            .basic_get(queue, lapin::options::BasicGetOptions { no_ack: true })
            .await
            .unwrap();
        if let Some(message) = message {
            return Some(message.data.clone());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
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
    delete_queue_family(&config, &service, RoundTripEvent::ROUTING_KEY).await;
}

#[tokio::test]
async fn mensagem_com_falha_volta_pela_fila_de_retry() {
    let metrics = shared::metrics::init();
    let mut config = AmqpConfig::from_env().expect("AMQP_URL ausente (suba o docker-compose)");
    config.retry_ttl_ms = 300;
    let bus = EventBus::connect(&config)
        .await
        .expect("broker inacessível");

    let service = format!("test-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Uuid>(4);
    let attempts = Arc::new(AtomicU32::new(0));

    let counter = attempts.clone();
    let consumer = bus
        .spawn_consumer::<RetryEvent, _, _>(&service, move |event: RetryEvent| {
            let tx = tx.clone();
            let counter = counter.clone();
            async move {
                let attempt = counter.fetch_add(1, Ordering::SeqCst) + 1;
                tx.send(event.id).await.unwrap();
                if attempt == 1 {
                    Err("falha proposital na primeira tentativa".into())
                } else {
                    Ok(())
                }
            }
        })
        .await
        .unwrap();

    let event = RetryEvent {
        id: Uuid::new_v4(),
        message: "vou falhar uma vez".to_owned(),
    };
    bus.publish(&event).await.unwrap();

    assert_eq!(recv(&mut rx).await, event.id);
    assert_eq!(recv(&mut rx).await, event.id, "não voltou pela fila retry");
    assert_eq!(attempts.load(Ordering::SeqCst), 2);

    let extra = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;
    assert!(extra.is_err(), "mensagem voltou depois de ter sido aceita");
    assert!(has_series(
        &metrics.render(),
        "message_retry_total",
        &[&format!("queue=\"{service}.{}\"", RetryEvent::ROUTING_KEY)]
    ));

    consumer.abort();
    delete_queue_family(&config, &service, RetryEvent::ROUTING_KEY).await;
}

#[tokio::test]
async fn mensagem_vai_para_a_fila_dead_apos_o_limite() {
    let metrics = shared::metrics::init();
    let mut config = AmqpConfig::from_env().expect("AMQP_URL ausente (suba o docker-compose)");
    config.retry_ttl_ms = 200;
    config.max_attempts = 2;
    let bus = EventBus::connect(&config)
        .await
        .expect("broker inacessível");

    let service = format!("test-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Uuid>(8);

    let consumer = bus
        .spawn_consumer::<DeadEvent, _, _>(&service, move |event: DeadEvent| {
            let tx = tx.clone();
            async move {
                tx.send(event.id).await.unwrap();
                Err("falha permanente".into())
            }
        })
        .await
        .unwrap();

    let event = DeadEvent {
        id: Uuid::new_v4(),
        message: "vou morrer".to_owned(),
    };
    bus.publish(&event).await.unwrap();

    assert_eq!(recv(&mut rx).await, event.id);
    assert_eq!(recv(&mut rx).await, event.id);

    let dead_queue = format!("{service}.{}.dead", DeadEvent::ROUTING_KEY);
    let payload = wait_for_dead_message(&config, &dead_queue)
        .await
        .expect("mensagem não chegou na fila dead");
    let parked: DeadEvent = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parked, event);

    let extra = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;
    assert!(extra.is_err(), "tentou processar além do limite");
    assert!(has_series(
        &metrics.render(),
        "message_dead_total",
        &[&format!("queue=\"{service}.{}\"", DeadEvent::ROUTING_KEY)]
    ));

    consumer.abort();
    delete_queue_family(&config, &service, DeadEvent::ROUTING_KEY).await;
}
