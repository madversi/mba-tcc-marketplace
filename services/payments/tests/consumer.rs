use std::time::Duration;

use domain::events::{Event, PaymentApproved, StockReserved};
use domain::{Money, Payment, PaymentStatus};
use payments::consumer;
use payments::gateway::SimulatedGateway;
use payments::repository::PaymentRepository;
use shared::{AmqpConfig, EventBus};
use sqlx::PgPool;
use uuid::Uuid;

async fn event_bus() -> EventBus {
    let config = AmqpConfig::from_env().expect("AMQP_URL ausente (suba o docker-compose)");
    EventBus::connect(&config)
        .await
        .expect("broker inacessível")
}

async fn delete_queue(name: &str) {
    let config = AmqpConfig::from_env().unwrap();
    let conn = lapin::Connection::connect(&config.url, lapin::ConnectionProperties::default())
        .await
        .unwrap();
    conn.create_channel()
        .await
        .unwrap()
        .queue_delete(name, lapin::options::QueueDeleteOptions::default())
        .await
        .unwrap();
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

async fn recv_matching(
    rx: &mut tokio::sync::mpsc::Receiver<PaymentApproved>,
    order_id: Uuid,
    payment_id: Uuid,
) -> PaymentApproved {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("payment.approved não chegou em 5s")
            .unwrap();
        if event.order_id == order_id && event.payment_id == payment_id {
            return event;
        }
    }
}

#[sqlx::test]
async fn cobra_e_publica_payment_approved(pool: PgPool) {
    let payments_repo = PaymentRepository::new(pool);
    let bus = event_bus().await;
    let service = format!("test-payments-{}", Uuid::new_v4());
    let consumer = consumer::spawn(
        bus.clone(),
        payments_repo.clone(),
        SimulatedGateway,
        &service,
    )
    .await
    .unwrap();

    let watcher_service = format!("test-watcher-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<PaymentApproved>(32);
    let watcher = bus
        .spawn_consumer::<PaymentApproved, _, _>(&watcher_service, move |event| {
            let tx = tx.clone();
            async move {
                tx.send(event).await.unwrap();
                Ok(())
            }
        })
        .await
        .unwrap();

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

    let received = recv_matching(&mut rx, order_id, payment.id).await;
    assert_eq!(received.payment_id, payment.id);

    consumer.abort();
    watcher.abort();
    delete_queue(&format!("{service}.{}", StockReserved::ROUTING_KEY)).await;
    delete_queue(&format!(
        "{watcher_service}.{}",
        PaymentApproved::ROUTING_KEY
    ))
    .await;
}

#[sqlx::test]
async fn evento_reentregue_nao_cobra_duas_vezes(pool: PgPool) {
    let payments_repo = PaymentRepository::new(pool);
    let bus = event_bus().await;
    let service = format!("test-payments-{}", Uuid::new_v4());
    let consumer = consumer::spawn(
        bus.clone(),
        payments_repo.clone(),
        SimulatedGateway,
        &service,
    )
    .await
    .unwrap();

    let watcher_service = format!("test-watcher-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<PaymentApproved>(32);
    let watcher = bus
        .spawn_consumer::<PaymentApproved, _, _>(&watcher_service, move |event| {
            let tx = tx.clone();
            async move {
                tx.send(event).await.unwrap();
                Ok(())
            }
        })
        .await
        .unwrap();

    let order_id = Uuid::new_v4();
    let event = StockReserved {
        order_id,
        amount: Money::from_cents(1_000),
        occurred_at: domain::time::now(),
    };

    bus.publish(&event).await.unwrap();
    let payment = wait_for_payment(&payments_repo, order_id).await;
    recv_matching(&mut rx, order_id, payment.id).await;

    bus.publish(&event).await.unwrap();
    let second = recv_matching(&mut rx, order_id, payment.id).await;
    assert_eq!(second.payment_id, payment.id);

    let still = payments_repo
        .find_by_order(order_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(still.id, payment.id);

    consumer.abort();
    watcher.abort();
    delete_queue(&format!("{service}.{}", StockReserved::ROUTING_KEY)).await;
    delete_queue(&format!(
        "{watcher_service}.{}",
        PaymentApproved::ROUTING_KEY
    ))
    .await;
}
