use domain::events::{PaymentApproved, PaymentFailed, StockReserved};
use domain::{Money, Payment, PaymentStatus};
use shared::amqp::{AmqpError, HandlerError};
use shared::EventBus;
use uuid::Uuid;

use crate::gateway::{ChargeOutcome, GatewayError, SimulatedGateway};
use crate::repository::PaymentRepository;

pub async fn spawn(
    bus: EventBus,
    payments: PaymentRepository,
    gateway: SimulatedGateway,
    service: &str,
) -> Result<tokio::task::JoinHandle<()>, AmqpError> {
    let publisher = bus.clone();
    bus.spawn_consumer::<StockReserved, _, _>(service, move |event| {
        let bus = publisher.clone();
        let payments = payments.clone();
        let gateway = gateway.clone();
        async move { handle(&bus, &payments, &gateway, event).await }
    })
    .await
}

async fn handle(
    bus: &EventBus,
    payments: &PaymentRepository,
    gateway: &SimulatedGateway,
    event: StockReserved,
) -> Result<(), HandlerError> {
    let payment = match payments.find_by_order(event.order_id).await? {
        Some(existing) => existing,
        None => charge_new(payments, gateway, event.order_id, event.amount).await?,
    };

    publish_result(bus, event.order_id, &payment).await
}

async fn charge_new(
    payments: &PaymentRepository,
    gateway: &SimulatedGateway,
    order_id: Uuid,
    amount: Money,
) -> Result<Payment, HandlerError> {
    let mut payment = Payment::new(order_id, amount.cents())?;
    payments.insert(&payment).await?;

    match gateway.charge(&payment).await {
        Ok(ChargeOutcome::Approved) => payment.approve()?,
        Ok(ChargeOutcome::Declined { reason }) => payment.fail(reason)?,
        Err(GatewayError::Unavailable) => return Ok(payment),
    }
    payments.update_status(&payment).await?;
    Ok(payment)
}

async fn publish_result(
    bus: &EventBus,
    order_id: Uuid,
    payment: &Payment,
) -> Result<(), HandlerError> {
    match payment.status {
        PaymentStatus::Approved => {
            bus.publish(&PaymentApproved {
                order_id,
                payment_id: payment.id,
                occurred_at: domain::time::now(),
            })
            .await?;
        }
        PaymentStatus::Failed => {
            bus.publish(&PaymentFailed {
                order_id,
                reason: payment
                    .failure_reason
                    .clone()
                    .unwrap_or_else(|| "pagamento recusado".to_owned()),
                occurred_at: domain::time::now(),
            })
            .await?;
        }
        PaymentStatus::Pending => {
            eprintln!(
                "pagamento {} do pedido {order_id} ficou PENDING (gateway indisponível)",
                payment.id
            );
        }
    }
    Ok(())
}
