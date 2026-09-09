use std::time::Duration;

use domain::events::{
    Event, OrderCreated, PaymentApproved, PaymentFailed, StockRejected, StockReserved,
};
use domain::{Money, OrderItem};
use inventory::consumer;
use inventory::repository::StockRepository;
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

async fn delete_consumer_queues(service: &str) {
    delete_queue(&format!("{service}.{}", OrderCreated::ROUTING_KEY)).await;
    delete_queue(&format!("{service}.{}", PaymentApproved::ROUTING_KEY)).await;
    delete_queue(&format!("{service}.{}", PaymentFailed::ROUTING_KEY)).await;
}

fn order_created(items: Vec<OrderItem>, total_cents: i64) -> OrderCreated {
    OrderCreated {
        order_id: Uuid::new_v4(),
        buyer_id: Uuid::new_v4(),
        items,
        total: Money::from_cents(total_cents),
        occurred_at: domain::time::now(),
    }
}

async fn recv_for<T>(rx: &mut tokio::sync::mpsc::Receiver<T>, order_id: Uuid) -> T
where
    T: HasOrderId,
{
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("evento não chegou em 5s")
            .unwrap();
        if event.order_id() == order_id {
            return event;
        }
    }
}

trait HasOrderId {
    fn order_id(&self) -> Uuid;
}

impl HasOrderId for StockReserved {
    fn order_id(&self) -> Uuid {
        self.order_id
    }
}

impl HasOrderId for StockRejected {
    fn order_id(&self) -> Uuid {
        self.order_id
    }
}

#[sqlx::test]
async fn reserva_estoque_e_publica_stock_reserved(pool: PgPool) {
    let stock = StockRepository::new(pool);
    let product_id = Uuid::new_v4();
    stock.set_available(product_id, 10).await.unwrap();

    let bus = event_bus().await;
    let service = format!("test-inventory-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), stock.clone(), &service)
        .await
        .unwrap();

    let watcher_service = format!("test-watcher-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StockReserved>(16);
    let watcher = bus
        .spawn_consumer::<StockReserved, _, _>(&watcher_service, move |event| {
            let tx = tx.clone();
            async move {
                tx.send(event).await.unwrap();
                Ok(())
            }
        })
        .await
        .unwrap();

    let event = order_created(
        vec![OrderItem::new(product_id, 3, Money::from_cents(1_000)).unwrap()],
        3_000,
    );
    let order_id = event.order_id;
    bus.publish(&event).await.unwrap();

    let received = recv_for(&mut rx, order_id).await;
    assert_eq!(received.amount, Money::from_cents(3_000));

    let item = stock.find(product_id).await.unwrap().unwrap();
    assert_eq!((item.available, item.reserved), (7, 3));
    assert_eq!(
        stock.find_reservation(order_id, product_id).await.unwrap(),
        Some(3)
    );

    for handle in handles {
        handle.abort();
    }
    watcher.abort();
    delete_consumer_queues(&service).await;
    delete_queue(&format!("{watcher_service}.{}", StockReserved::ROUTING_KEY)).await;
}

#[sqlx::test]
async fn estoque_insuficiente_publica_stock_rejected(pool: PgPool) {
    let stock = StockRepository::new(pool);
    let product_id = Uuid::new_v4();
    stock.set_available(product_id, 1).await.unwrap();

    let bus = event_bus().await;
    let service = format!("test-inventory-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), stock.clone(), &service)
        .await
        .unwrap();

    let watcher_service = format!("test-watcher-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StockRejected>(16);
    let watcher = bus
        .spawn_consumer::<StockRejected, _, _>(&watcher_service, move |event| {
            let tx = tx.clone();
            async move {
                tx.send(event).await.unwrap();
                Ok(())
            }
        })
        .await
        .unwrap();

    let event = order_created(
        vec![OrderItem::new(product_id, 5, Money::from_cents(1_000)).unwrap()],
        5_000,
    );
    let order_id = event.order_id;
    bus.publish(&event).await.unwrap();

    let received = recv_for(&mut rx, order_id).await;
    assert!(
        received.reason.contains("insuficiente"),
        "{}",
        received.reason
    );

    let item = stock.find(product_id).await.unwrap().unwrap();
    assert_eq!((item.available, item.reserved), (1, 0));

    for handle in handles {
        handle.abort();
    }
    watcher.abort();
    delete_consumer_queues(&service).await;
    delete_queue(&format!("{watcher_service}.{}", StockRejected::ROUTING_KEY)).await;
}

#[sqlx::test]
async fn pedido_multi_item_e_atomico_via_evento(pool: PgPool) {
    let stock = StockRepository::new(pool);
    let ok_product = Uuid::new_v4();
    let short_product = Uuid::new_v4();
    stock.set_available(ok_product, 10).await.unwrap();
    stock.set_available(short_product, 1).await.unwrap();

    let bus = event_bus().await;
    let service = format!("test-inventory-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), stock.clone(), &service)
        .await
        .unwrap();

    let watcher_service = format!("test-watcher-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StockRejected>(16);
    let watcher = bus
        .spawn_consumer::<StockRejected, _, _>(&watcher_service, move |event| {
            let tx = tx.clone();
            async move {
                tx.send(event).await.unwrap();
                Ok(())
            }
        })
        .await
        .unwrap();

    let event = order_created(
        vec![
            OrderItem::new(ok_product, 5, Money::from_cents(1_000)).unwrap(),
            OrderItem::new(short_product, 5, Money::from_cents(500)).unwrap(),
        ],
        7_500,
    );
    let order_id = event.order_id;
    bus.publish(&event).await.unwrap();

    recv_for(&mut rx, order_id).await;

    let ok_item = stock.find(ok_product).await.unwrap().unwrap();
    assert_eq!((ok_item.available, ok_item.reserved), (10, 0));
    assert_eq!(
        stock.find_reservation(order_id, ok_product).await.unwrap(),
        None
    );

    for handle in handles {
        handle.abort();
    }
    watcher.abort();
    delete_consumer_queues(&service).await;
    delete_queue(&format!("{watcher_service}.{}", StockRejected::ROUTING_KEY)).await;
}

#[sqlx::test]
async fn produto_sem_estoque_cadastrado_publica_stock_rejected(pool: PgPool) {
    let stock = StockRepository::new(pool);
    let product_id = Uuid::new_v4();

    let bus = event_bus().await;
    let service = format!("test-inventory-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), stock.clone(), &service)
        .await
        .unwrap();

    let watcher_service = format!("test-watcher-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StockRejected>(16);
    let watcher = bus
        .spawn_consumer::<StockRejected, _, _>(&watcher_service, move |event| {
            let tx = tx.clone();
            async move {
                tx.send(event).await.unwrap();
                Ok(())
            }
        })
        .await
        .unwrap();

    let event = order_created(
        vec![OrderItem::new(product_id, 1, Money::from_cents(100)).unwrap()],
        100,
    );
    let order_id = event.order_id;
    bus.publish(&event).await.unwrap();

    let received = recv_for(&mut rx, order_id).await;
    assert_eq!(received.reason, "produto sem estoque cadastrado");

    for handle in handles {
        handle.abort();
    }
    watcher.abort();
    delete_consumer_queues(&service).await;
    delete_queue(&format!("{watcher_service}.{}", StockRejected::ROUTING_KEY)).await;
}

async fn wait_for_reservation(
    stock: &StockRepository,
    order_id: Uuid,
    product_id: Uuid,
) -> Option<u32> {
    for _ in 0..30 {
        stock.find_reservation(order_id, product_id).await.unwrap()?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stock.find_reservation(order_id, product_id).await.unwrap()
}

#[sqlx::test]
async fn payment_failed_libera_a_reserva(pool: PgPool) {
    let stock = StockRepository::new(pool);
    let product_id = Uuid::new_v4();
    stock.set_available(product_id, 10).await.unwrap();

    let order_id = Uuid::new_v4();
    stock
        .reserve_many(order_id, &[(product_id, 4)])
        .await
        .unwrap();

    let bus = event_bus().await;
    let service = format!("test-inventory-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), stock.clone(), &service)
        .await
        .unwrap();

    bus.publish(&PaymentFailed {
        order_id,
        reason: "cartão recusado".to_owned(),
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();

    let remaining = wait_for_reservation(&stock, order_id, product_id).await;
    assert_eq!(remaining, None);
    let item = stock.find(product_id).await.unwrap().unwrap();
    assert_eq!((item.available, item.reserved), (10, 0));

    for handle in handles {
        handle.abort();
    }
    delete_consumer_queues(&service).await;
}

#[sqlx::test]
async fn payment_approved_consome_a_reserva(pool: PgPool) {
    let stock = StockRepository::new(pool);
    let product_id = Uuid::new_v4();
    stock.set_available(product_id, 10).await.unwrap();

    let order_id = Uuid::new_v4();
    stock
        .reserve_many(order_id, &[(product_id, 4)])
        .await
        .unwrap();

    let bus = event_bus().await;
    let service = format!("test-inventory-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), stock.clone(), &service)
        .await
        .unwrap();

    bus.publish(&PaymentApproved {
        order_id,
        payment_id: Uuid::new_v4(),
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();

    let remaining = wait_for_reservation(&stock, order_id, product_id).await;
    assert_eq!(remaining, None);
    let item = stock.find(product_id).await.unwrap().unwrap();
    assert_eq!((item.available, item.reserved), (6, 0));

    for handle in handles {
        handle.abort();
    }
    delete_consumer_queues(&service).await;
}
