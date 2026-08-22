use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use domain::Payment;
use serde::Deserialize;
use shared::ApiError;
use uuid::Uuid;

use crate::app::AppState;
use crate::gateway::{ChargeOutcome, GatewayError};

#[derive(Deserialize)]
pub struct CreatePayment {
    pub order_id: Uuid,
    pub amount_cents: i64,
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreatePayment>,
) -> Result<(StatusCode, Json<Payment>), ApiError> {
    let mut payment = Payment::new(body.order_id, body.amount_cents)?;
    state
        .payments
        .insert(&payment)
        .await
        .map_err(|err| match ApiError::from(err) {
            ApiError::Conflict(_) => ApiError::Conflict("pedido já possui pagamento".to_owned()),
            other => other,
        })?;

    match state.gateway.charge(&payment).await {
        Ok(ChargeOutcome::Approved) => payment.approve()?,
        Ok(ChargeOutcome::Declined { reason }) => payment.fail(reason)?,
        Err(GatewayError::Unavailable) => {
            return Err(ApiError::Unavailable(
                "gateway de pagamento indisponível; pagamento pendente".to_owned(),
            ))
        }
    }
    state.payments.update_status(&payment).await?;

    Ok((StatusCode::CREATED, Json(payment)))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Payment>, ApiError> {
    state
        .payments
        .find_by_id(id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound("pagamento"))
}

pub async fn get_by_order(
    State(state): State<AppState>,
    Path(order_id): Path<Uuid>,
) -> Result<Json<Payment>, ApiError> {
    state
        .payments
        .find_by_order(order_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound("pagamento do pedido"))
}
