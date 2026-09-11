use domain::events::{OrderCreated, PaymentApproved, PaymentFailed, StockRejected, StockReserved};
use shared::amqp::{AmqpError, HandlerError};
use shared::EventBus;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::repository::{StockError, StockRepository};

pub async fn spawn(
    bus: EventBus,
    stock: StockRepository,
    service: &str,
) -> Result<Vec<JoinHandle<()>>, AmqpError> {
    let mut handles = Vec::with_capacity(3);

    let publisher = bus.clone();
    let stock_ref = stock.clone();
    handles.push(
        bus.spawn_consumer::<OrderCreated, _, _>(service, move |event| {
            let bus = publisher.clone();
            let stock = stock_ref.clone();
            async move { handle_order_created(&bus, &stock, event).await }
        })
        .await?,
    );

    let stock_ref = stock.clone();
    handles.push(
        bus.spawn_consumer::<PaymentApproved, _, _>(service, move |event| {
            let stock = stock_ref.clone();
            async move {
                stock.commit_reserved(event.order_id).await?;
                Ok(())
            }
        })
        .await?,
    );

    let stock_ref = stock.clone();
    handles.push(
        bus.spawn_consumer::<PaymentFailed, _, _>(service, move |event| {
            let stock = stock_ref.clone();
            async move {
                stock.release_reserved(event.order_id).await?;
                Ok(())
            }
        })
        .await?,
    );

    Ok(handles)
}

async fn handle_order_created(
    bus: &EventBus,
    stock: &StockRepository,
    event: OrderCreated,
) -> Result<(), HandlerError> {
    let items: Vec<(Uuid, u32)> = event
        .items
        .iter()
        .map(|item| (item.product_id, item.quantity))
        .collect();

    match stock.reserve_many(event.order_id, &items).await {
        Ok(()) => {
            bus.publish(&StockReserved {
                order_id: event.order_id,
                amount: event.total,
                occurred_at: domain::time::now(),
            })
            .await?;
        }
        Err(err) => {
            bus.publish(&StockRejected {
                order_id: event.order_id,
                reason: reject_reason(err),
                occurred_at: domain::time::now(),
            })
            .await?;
        }
    }
    Ok(())
}

fn reject_reason(err: StockError) -> String {
    match err {
        StockError::NotFound => "produto sem estoque cadastrado".to_owned(),
        StockError::Domain(e) => e.to_string(),
        StockError::Db(e) => {
            tracing::error!(err = %e, "erro de banco ao reservar estoque");
            "erro interno ao reservar estoque".to_owned()
        }
    }
}
