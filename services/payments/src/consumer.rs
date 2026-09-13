use std::time::Duration;

use domain::events::{PaymentApproved, PaymentFailed, PaymentPending, StockReserved};
use domain::{Payment, PaymentStatus};
use shared::amqp::{AmqpError, HandlerError};
use shared::config::{env_vars, parse_or};
use shared::{ConfigError, EnvVars, EventBus};
use tokio::task::JoinHandle;

use crate::gateway::{ChargeOutcome, GatewayClient, GatewayError};
use crate::repository::PaymentRepository;

#[derive(Debug, Clone, Copy)]
pub struct Reprocessing {
    pub interval: Duration,
    pub timeout: Duration,
}

impl Reprocessing {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_source(&env_vars())
    }

    pub fn from_source(vars: &EnvVars) -> Result<Self, ConfigError> {
        Ok(Self {
            interval: Duration::from_millis(parse_or(
                vars,
                "PAYMENT_REPROCESS_INTERVAL_MS",
                5_000,
            )?),
            timeout: Duration::from_secs(parse_or(vars, "PAYMENT_REPROCESS_TIMEOUT_SECS", 300)?),
        })
    }

    fn max_attempts(&self) -> u32 {
        let rounds = self.timeout.as_millis() / self.interval.as_millis().max(1);
        u32::try_from(rounds).unwrap_or(u32::MAX).saturating_add(2)
    }

    fn expired(&self, payment: &Payment) -> bool {
        (domain::time::now() - payment.created_at)
            .to_std()
            .is_ok_and(|elapsed| elapsed >= self.timeout)
    }
}

enum Attempt {
    Settled,
    GatewayDown(GatewayError),
}

pub async fn spawn(
    bus: EventBus,
    payments: PaymentRepository,
    gateway: GatewayClient,
    reprocessing: Reprocessing,
    service: &str,
) -> Result<Vec<JoinHandle<()>>, AmqpError> {
    let mut handles = Vec::with_capacity(2);

    let publisher = bus.clone();
    let repo = payments.clone();
    let client = gateway.clone();
    handles.push(
        bus.spawn_consumer::<StockReserved, _, _>(service, move |event| {
            let bus = publisher.clone();
            let payments = repo.clone();
            let gateway = client.clone();
            async move { handle_stock_reserved(&bus, &payments, &gateway, event).await }
        })
        .await?,
    );

    let reprocess_bus = bus.with_retry(
        reprocessing.interval.as_millis() as u64,
        reprocessing.max_attempts(),
    );
    let publisher = bus.clone();
    handles.push(
        reprocess_bus
            .spawn_consumer::<PaymentPending, _, _>(service, move |event| {
                let bus = publisher.clone();
                let payments = payments.clone();
                let gateway = gateway.clone();
                async move { reprocess(&bus, &payments, &gateway, reprocessing, event).await }
            })
            .await?,
    );

    Ok(handles)
}

async fn handle_stock_reserved(
    bus: &EventBus,
    payments: &PaymentRepository,
    gateway: &GatewayClient,
    event: StockReserved,
) -> Result<(), HandlerError> {
    let mut payment = match payments.find_by_order(event.order_id).await? {
        Some(existing) => existing,
        None => {
            let payment = Payment::new(event.order_id, event.amount.cents())?;
            payments.insert(&payment).await?;
            payment
        }
    };

    if payment.status != PaymentStatus::Pending {
        return publish_result(bus, &payment).await;
    }

    match attempt_charge(payments, gateway, &mut payment).await? {
        Attempt::Settled => publish_result(bus, &payment).await,
        Attempt::GatewayDown(err) => {
            tracing::warn!(
                payment_id = %payment.id,
                order_id = %payment.order_id,
                %err,
                "pagamento pendente; enviado para reprocessamento"
            );
            bus.publish(&PaymentPending {
                order_id: payment.order_id,
                payment_id: payment.id,
                occurred_at: domain::time::now(),
            })
            .await?;
            Ok(())
        }
    }
}

async fn reprocess(
    bus: &EventBus,
    payments: &PaymentRepository,
    gateway: &GatewayClient,
    reprocessing: Reprocessing,
    event: PaymentPending,
) -> Result<(), HandlerError> {
    let Some(mut payment) = payments.find_by_id(event.payment_id).await? else {
        return Ok(());
    };

    if payment.status != PaymentStatus::Pending {
        return publish_result(bus, &payment).await;
    }

    match attempt_charge(payments, gateway, &mut payment).await? {
        Attempt::Settled => {
            record_reprocessing(&payment, "recovered");
            tracing::info!(
                payment_id = %payment.id,
                status = %payment.status,
                "pagamento reprocessado"
            );
            publish_result(bus, &payment).await
        }
        Attempt::GatewayDown(err) if reprocessing.expired(&payment) => {
            record_reprocessing(&payment, "expired");
            tracing::error!(
                payment_id = %payment.id,
                order_id = %payment.order_id,
                %err,
                "reprocessamento esgotado; pagamento falhou"
            );
            payment.fail(format!(
                "gateway indisponível por mais de {}s",
                reprocessing.timeout.as_secs()
            ))?;
            payments.update_status(&payment).await?;
            publish_result(bus, &payment).await
        }
        Attempt::GatewayDown(err) => Err(err.into()),
    }
}

fn record_reprocessing(payment: &Payment, outcome: &'static str) {
    let elapsed = (domain::time::now() - payment.created_at)
        .to_std()
        .unwrap_or_default();
    metrics::histogram!("failure_reprocessing_duration_seconds", "outcome" => outcome)
        .record(elapsed.as_secs_f64());
}

async fn attempt_charge(
    payments: &PaymentRepository,
    gateway: &GatewayClient,
    payment: &mut Payment,
) -> Result<Attempt, HandlerError> {
    match gateway.charge(payment).await {
        Ok(ChargeOutcome::Approved) => payment.approve()?,
        Ok(ChargeOutcome::Declined { reason }) => payment.fail(reason)?,
        Err(err) => return Ok(Attempt::GatewayDown(err)),
    }
    payments.update_status(payment).await?;
    Ok(Attempt::Settled)
}

async fn publish_result(bus: &EventBus, payment: &Payment) -> Result<(), HandlerError> {
    match payment.status {
        PaymentStatus::Approved => {
            bus.publish(&PaymentApproved {
                order_id: payment.order_id,
                payment_id: payment.id,
                occurred_at: domain::time::now(),
            })
            .await?;
        }
        PaymentStatus::Failed => {
            bus.publish(&PaymentFailed {
                order_id: payment.order_id,
                reason: payment
                    .failure_reason
                    .clone()
                    .unwrap_or_else(|| "pagamento recusado".to_owned()),
                occurred_at: domain::time::now(),
            })
            .await?;
        }
        PaymentStatus::Pending => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reprocessamento_le_defaults() {
        let cfg = Reprocessing::from_source(&EnvVars::new()).unwrap();

        assert_eq!(cfg.interval, Duration::from_millis(5_000));
        assert_eq!(cfg.timeout, Duration::from_secs(300));
    }

    #[test]
    fn limite_da_fila_retry_nao_vence_antes_do_prazo() {
        let cfg = Reprocessing {
            interval: Duration::from_secs(5),
            timeout: Duration::from_secs(300),
        };

        assert!(cfg.max_attempts() as u64 * 5 > 300);
    }
}
