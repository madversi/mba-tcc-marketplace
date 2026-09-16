use std::time::Duration;

use domain::events::{Event, PaymentApproved, PaymentFailed, PaymentPending, StockReserved};
use domain::{Money, Payment, PaymentStatus};
use payments::consumer::{self, Reprocessing};
use payments::gateway::{GatewayClient, GatewayResilience, SimulatedGateway};
use payments::repository::PaymentRepository;
use shared::testing::IsolatedBroker;
use shared::{AmqpConfig, EventBus};
use sqlx::PgPool;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use uuid::Uuid;

async fn delete_queue(name: &str) {
    let config = AmqpConfig::from_env().unwrap();
    let channel = lapin::Connection::connect(&config.url, lapin::ConnectionProperties::default())
        .await
        .unwrap()
        .create_channel()
        .await
        .unwrap();
    for queue in [
        name.to_owned(),
        format!("{name}.retry"),
        format!("{name}.dead"),
    ] {
        channel
            .queue_delete(&queue, lapin::options::QueueDeleteOptions::default())
            .await
            .unwrap();
    }
}

async fn delete_consumer_queues(service: &str) {
    delete_queue(&format!("{service}.{}", StockReserved::ROUTING_KEY)).await;
    delete_queue(&format!("{service}.{}", PaymentPending::ROUTING_KEY)).await;
}

fn client(gateway: SimulatedGateway) -> GatewayClient {
    GatewayClient::new(
        gateway,
        GatewayResilience {
            breaker_failure_threshold: 5,
            breaker_open_timeout: Duration::from_millis(200),
        },
    )
}

fn reprocessing(interval_ms: u64, timeout_ms: u64) -> Reprocessing {
    Reprocessing {
        interval: Duration::from_millis(interval_ms),
        timeout: Duration::from_millis(timeout_ms),
    }
}

async fn watch<E: Event + 'static>(bus: &EventBus) -> (Receiver<E>, JoinHandle<()>, String) {
    let service = format!("test-watcher-{}", Uuid::new_v4());
    let (tx, rx) = tokio::sync::mpsc::channel::<E>(32);
    let handle = bus
        .spawn_consumer::<E, _, _>(&service, move |event| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(event).await;
                Ok(())
            }
        })
        .await
        .unwrap();
    (rx, handle, format!("{service}.{}", E::ROUTING_KEY))
}

fn has_series(metrics: &str, name: &str, labels: &[&str]) -> bool {
    metrics
        .lines()
        .any(|line| line.starts_with(name) && labels.iter().all(|label| line.contains(label)))
}

async fn recv<E>(rx: &mut Receiver<E>) -> E {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("evento não chegou em 10s")
        .unwrap()
}

async fn wait_for_payment(repo: &PaymentRepository, order_id: Uuid) -> Payment {
    for _ in 0..50 {
        if let Some(payment) = repo.find_by_order(order_id).await.unwrap() {
            return payment;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("pagamento do pedido {order_id} não foi processado em 5s");
}

#[sqlx::test]
async fn cobra_e_publica_payment_approved(pool: PgPool) {
    let payments_repo = PaymentRepository::new(pool);
    let broker = IsolatedBroker::connect().await;
    let bus = broker.bus();
    let service = format!("test-payments-{}", Uuid::new_v4());
    let handles = consumer::spawn(
        bus.clone(),
        payments_repo.clone(),
        client(SimulatedGateway::default()),
        reprocessing(5_000, 300_000),
        &service,
    )
    .await
    .unwrap();

    let (mut rx, watcher, watcher_queue) = watch::<PaymentApproved>(&bus).await;

    let order_id = Uuid::new_v4();
    bus.publish(&StockReserved {
        order_id,
        amount: Money::from_cents(2_500),
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();

    let payment = wait_for_payment(&payments_repo, order_id).await;
    assert_eq!(payment.amount, Money::from_cents(2_500));
    assert_eq!(payment.status, PaymentStatus::Approved);

    let received = recv(&mut rx).await;
    assert_eq!(received.order_id, order_id);
    assert_eq!(received.payment_id, payment.id);

    for handle in handles {
        handle.abort();
    }
    watcher.abort();
    delete_consumer_queues(&service).await;
    delete_queue(&watcher_queue).await;
}

#[sqlx::test]
async fn evento_reentregue_nao_cobra_duas_vezes(pool: PgPool) {
    let payments_repo = PaymentRepository::new(pool);
    let broker = IsolatedBroker::connect().await;
    let bus = broker.bus();
    let service = format!("test-payments-{}", Uuid::new_v4());
    let handles = consumer::spawn(
        bus.clone(),
        payments_repo.clone(),
        client(SimulatedGateway::default()),
        reprocessing(5_000, 300_000),
        &service,
    )
    .await
    .unwrap();

    let (mut rx, watcher, watcher_queue) = watch::<PaymentApproved>(&bus).await;

    let order_id = Uuid::new_v4();
    let event = StockReserved {
        order_id,
        amount: Money::from_cents(1_000),
        occurred_at: domain::time::now(),
    };

    bus.publish(&event).await.unwrap();
    let payment = wait_for_payment(&payments_repo, order_id).await;
    let first = recv(&mut rx).await;
    assert_eq!(first.payment_id, payment.id);

    bus.publish(&event).await.unwrap();
    let second = recv(&mut rx).await;
    assert_eq!(second.payment_id, payment.id);

    let still = payments_repo
        .find_by_order(order_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(still.id, payment.id);

    for handle in handles {
        handle.abort();
    }
    watcher.abort();
    delete_consumer_queues(&service).await;
    delete_queue(&watcher_queue).await;
}

#[sqlx::test]
async fn gateway_indisponivel_reprocessa_e_aprova_quando_volta(pool: PgPool) {
    let metrics = shared::metrics::init();
    let payments_repo = PaymentRepository::new(pool);
    let broker = IsolatedBroker::connect().await;
    let bus = broker.bus();
    let gateway = SimulatedGateway::default();
    gateway.set_unavailable(true);

    let service = format!("test-payments-{}", Uuid::new_v4());
    let handles = consumer::spawn(
        bus.clone(),
        payments_repo.clone(),
        client(gateway.clone()),
        reprocessing(300, 30_000),
        &service,
    )
    .await
    .unwrap();
    let (mut pending_rx, pending_watcher, pending_queue) = watch::<PaymentPending>(&bus).await;
    let (mut approved_rx, approved_watcher, approved_queue) = watch::<PaymentApproved>(&bus).await;

    let order_id = Uuid::new_v4();
    bus.publish(&StockReserved {
        order_id,
        amount: Money::from_cents(1_500),
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();

    let pending = recv(&mut pending_rx).await;
    assert_eq!(pending.order_id, order_id);
    let payment = payments_repo
        .find_by_id(pending.payment_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(payment.status, PaymentStatus::Pending);

    gateway.set_unavailable(false);

    let approved = recv(&mut approved_rx).await;
    assert_eq!(approved.payment_id, pending.payment_id);
    let settled = payments_repo
        .find_by_id(pending.payment_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(settled.status, PaymentStatus::Approved);
    assert!(has_series(
        &metrics.render(),
        "failure_reprocessing_duration_seconds_bucket",
        &["outcome=\"recovered\""]
    ));

    for handle in handles {
        handle.abort();
    }
    pending_watcher.abort();
    approved_watcher.abort();
    delete_consumer_queues(&service).await;
    delete_queue(&pending_queue).await;
    delete_queue(&approved_queue).await;
}

#[sqlx::test]
async fn reprocessamento_esgotado_falha_o_pagamento(pool: PgPool) {
    let metrics = shared::metrics::init();
    let payments_repo = PaymentRepository::new(pool);
    let broker = IsolatedBroker::connect().await;
    let bus = broker.bus();
    let gateway = SimulatedGateway::default();
    gateway.set_unavailable(true);

    let service = format!("test-payments-{}", Uuid::new_v4());
    let handles = consumer::spawn(
        bus.clone(),
        payments_repo.clone(),
        client(gateway),
        reprocessing(200, 600),
        &service,
    )
    .await
    .unwrap();
    let (mut failed_rx, failed_watcher, failed_queue) = watch::<PaymentFailed>(&bus).await;

    let order_id = Uuid::new_v4();
    bus.publish(&StockReserved {
        order_id,
        amount: Money::from_cents(1_500),
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();

    let failed = recv(&mut failed_rx).await;
    assert_eq!(failed.order_id, order_id);
    assert!(
        failed.reason.contains("gateway indisponível"),
        "{}",
        failed.reason
    );

    let payment = payments_repo
        .find_by_order(order_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(payment.status, PaymentStatus::Failed);

    let rendered = metrics.render();
    assert!(has_series(
        &rendered,
        "failure_reprocessing_duration_seconds_bucket",
        &["outcome=\"expired\""]
    ));
    assert!(has_series(
        &rendered,
        "message_retry_total",
        &[&format!(
            "queue=\"{service}.{}\"",
            PaymentPending::ROUTING_KEY
        )]
    ));

    for handle in handles {
        handle.abort();
    }
    failed_watcher.abort();
    delete_consumer_queues(&service).await;
    delete_queue(&failed_queue).await;
}
