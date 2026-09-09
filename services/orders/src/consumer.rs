use domain::events::{PaymentApproved, PaymentFailed, StockRejected, StockReserved};
use domain::{DomainError, Order};
use shared::amqp::{AmqpError, HandlerError};
use shared::EventBus;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::repository::{OrderError, OrderRepository};

pub async fn spawn(
    bus: EventBus,
    orders: OrderRepository,
    service: &str,
) -> Result<Vec<JoinHandle<()>>, AmqpError> {
    let mut handles = Vec::with_capacity(4);

    let orders_ref = orders.clone();
    handles.push(
        bus.spawn_consumer::<StockReserved, _, _>(service, move |event| {
            let orders = orders_ref.clone();
            async move { apply(&orders, event.order_id, |o| o.mark_stock_reserved()).await }
        })
        .await?,
    );

    let orders_ref = orders.clone();
    handles.push(
        bus.spawn_consumer::<StockRejected, _, _>(service, move |event| {
            let orders = orders_ref.clone();
            async move { apply(&orders, event.order_id, |o| o.cancel()).await }
        })
        .await?,
    );

    let orders_ref = orders.clone();
    handles.push(
        bus.spawn_consumer::<PaymentApproved, _, _>(service, move |event| {
            let orders = orders_ref.clone();
            async move { apply(&orders, event.order_id, |o| o.confirm()).await }
        })
        .await?,
    );

    let orders_ref = orders.clone();
    handles.push(
        bus.spawn_consumer::<PaymentFailed, _, _>(service, move |event| {
            let orders = orders_ref.clone();
            async move { apply(&orders, event.order_id, |o| o.cancel()).await }
        })
        .await?,
    );

    Ok(handles)
}

async fn apply(
    orders: &OrderRepository,
    order_id: Uuid,
    transition: impl FnOnce(&mut Order) -> Result<(), DomainError>,
) -> Result<(), HandlerError> {
    match orders.transition(order_id, transition).await {
        Ok(_) => Ok(()),
        Err(OrderError::Domain(DomainError::InvalidTransition { from, to })) => {
            eprintln!("transição {from} -> {to} ignorada para o pedido {order_id}");
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}
