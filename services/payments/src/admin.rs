use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use shared::ApiError;

use crate::app::AppState;
use crate::gateway::GatewayConfigView;

#[derive(Deserialize, Default)]
pub struct UpdateGatewayConfig {
    pub failure_rate: Option<f64>,
    pub latency_ms: Option<u64>,
    pub unavailable: Option<bool>,
}

pub async fn get_gateway_config(State(state): State<AppState>) -> Json<GatewayConfigView> {
    Json(state.gateway.simulated().config_view())
}

pub async fn update_gateway_config(
    State(state): State<AppState>,
    Json(body): Json<UpdateGatewayConfig>,
) -> Result<Json<GatewayConfigView>, ApiError> {
    let gateway = state.gateway.simulated();
    if let Some(rate) = body.failure_rate {
        if !(0.0..=1.0).contains(&rate) {
            return Err(ApiError::Validation(
                "failure_rate deve estar entre 0.0 e 1.0".to_owned(),
            ));
        }
        gateway.set_failure_rate(rate);
    }
    if let Some(latency_ms) = body.latency_ms {
        gateway.set_latency_ms(latency_ms);
    }
    if let Some(unavailable) = body.unavailable {
        gateway.set_unavailable(unavailable);
    }

    Ok(Json(gateway.config_view()))
}
