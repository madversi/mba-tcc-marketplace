use std::time::Duration;

use domain::events::{Event, PaymentApproved, PaymentFailed, StockRejected, StockReserved};
use domain::{Money, Order, OrderItem, OrderStatus};
use orders::consumer;
use orders::repository::OrderRepository;
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
    delete_queue(&format!("{service}.{}", StockReserved::ROUTING_KEY)).await;
    delete_queue(&format!("{service}.{}", StockRejected::ROUTING_KEY)).await;
    delete_queue(&format!("{service}.{}", PaymentApproved::ROUTING_KEY)).await;
    delete_queue(&format!("{service}.{}", PaymentFailed::ROUTING_KEY)).await;
}

fn new_order() -> Order {
    let items = vec![OrderItem::new(Uuid::new_v4(), 1, Money::from_cents(1_000)).unwrap()];
    Order::new(Uuid::new_v4(), items).unwrap()
}

async fn wait_for_status(repo: &OrderRepository, order_id: Uuid, status: OrderStatus) -> Order {
    for _ in 0..50 {
        if let Some(order) = repo.find_by_id(order_id).await.unwrap() {
            if order.status == status {
                return order;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("pedido {order_id} não chegou em {status} em 5s");
}

#[sqlx::test]
async fn stock_reserved_atualiza_status(pool: PgPool) {
    let repo = OrderRepository::new(pool);
    let order = new_order();
    repo.insert(&order).await.unwrap();

    let bus = event_bus().await;
    let service = format!("test-orders-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), repo.clone(), &service)
        .await
        .unwrap();

    bus.publish(&StockReserved {
        order_id: order.id,
        amount: order.total,
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();

    wait_for_status(&repo, order.id, OrderStatus::StockReserved).await;

    for handle in handles {
        handle.abort();
    }
    delete_consumer_queues(&service).await;
}

#[sqlx::test]
async fn stock_rejected_cancela_pedido(pool: PgPool) {
    let repo = OrderRepository::new(pool);
    let order = new_order();
    repo.insert(&order).await.unwrap();

    let bus = event_bus().await;
    let service = format!("test-orders-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), repo.clone(), &service)
        .await
        .unwrap();

    bus.publish(&StockRejected {
        order_id: order.id,
        reason: "sem estoque".to_owned(),
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();

    wait_for_status(&repo, order.id, OrderStatus::Cancelled).await;

    for handle in handles {
        handle.abort();
    }
    delete_consumer_queues(&service).await;
}

#[sqlx::test]
async fn caminho_feliz_completo_ate_confirmed(pool: PgPool) {
    let repo = OrderRepository::new(pool);
    let order = new_order();
    repo.insert(&order).await.unwrap();

    let bus = event_bus().await;
    let service = format!("test-orders-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), repo.clone(), &service)
        .await
        .unwrap();

    bus.publish(&StockReserved {
        order_id: order.id,
        amount: order.total,
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();
    wait_for_status(&repo, order.id, OrderStatus::StockReserved).await;

    bus.publish(&PaymentApproved {
        order_id: order.id,
        payment_id: Uuid::new_v4(),
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();
    wait_for_status(&repo, order.id, OrderStatus::Confirmed).await;

    for handle in handles {
        handle.abort();
    }
    delete_consumer_queues(&service).await;
}

#[sqlx::test]
async fn stock_reserved_seguido_de_payment_failed_cancela(pool: PgPool) {
    let repo = OrderRepository::new(pool);
    let order = new_order();
    repo.insert(&order).await.unwrap();

    let bus = event_bus().await;
    let service = format!("test-orders-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), repo.clone(), &service)
        .await
        .unwrap();

    bus.publish(&StockReserved {
        order_id: order.id,
        amount: order.total,
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();
    wait_for_status(&repo, order.id, OrderStatus::StockReserved).await;

    bus.publish(&PaymentFailed {
        order_id: order.id,
        reason: "cartão recusado".to_owned(),
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();
    wait_for_status(&repo, order.id, OrderStatus::Cancelled).await;

    for handle in handles {
        handle.abort();
    }
    delete_consumer_queues(&service).await;
}

#[sqlx::test]
async fn payment_approved_antes_de_stock_reserved_ainda_confirma(pool: PgPool) {
    let repo = OrderRepository::new(pool);
    let order = new_order();
    repo.insert(&order).await.unwrap();

    let bus = event_bus().await;
    let service = format!("test-orders-{}", Uuid::new_v4());
    let handles = consumer::spawn(bus.clone(), repo.clone(), &service)
        .await
        .unwrap();

    bus.publish(&PaymentApproved {
        order_id: order.id,
        payment_id: Uuid::new_v4(),
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();
    wait_for_status(&repo, order.id, OrderStatus::Confirmed).await;

    bus.publish(&StockReserved {
        order_id: order.id,
        amount: order.total,
        occurred_at: domain::time::now(),
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = repo.find_by_id(order.id).await.unwrap().unwrap();
    assert_eq!(after.status, OrderStatus::Confirmed);

    for handle in handles {
        handle.abort();
    }
    delete_consumer_queues(&service).await;
}
