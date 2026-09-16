use std::time::Duration;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use domain::DomainError;
use serde::Serialize;
use sqlx::PgPool;
use tower_http::timeout::TimeoutLayer;

pub fn request_timeout(timeout: Duration) -> TimeoutLayer {
    TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, timeout)
}

#[derive(Debug)]
pub enum ApiError {
    NotFound(&'static str),
    Validation(String),
    Conflict(String),
    Unavailable(String),
    Internal(sqlx::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::NotFound(what) => (StatusCode::NOT_FOUND, format!("{what} não encontrado")),
            Self::Validation(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg),
            Self::Conflict(msg) => (StatusCode::CONFLICT, msg),
            Self::Unavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            Self::Internal(err) => {
                eprintln!("erro interno: {err}");
                (StatusCode::INTERNAL_SERVER_ERROR, "erro interno".to_owned())
            }
        };

        (status, Json(ErrorBody { error: message })).into_response()
    }
}

impl From<DomainError> for ApiError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::InsufficientStock { .. }
            | DomainError::ReleaseExceedsReserved { .. }
            | DomainError::InvalidTransition { .. } => Self::Conflict(err.to_string()),
            _ => Self::Validation(err.to_string()),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        match err.as_database_error() {
            Some(db) if db.is_unique_violation() => Self::Conflict("registro duplicado".to_owned()),
            Some(db) if db.is_foreign_key_violation() => {
                Self::Validation("referência a registro inexistente".to_owned())
            }
            _ => Self::Internal(err),
        }
    }
}

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    service: String,
    version: &'static str,
    database: &'static str,
}

pub async fn health(
    pool: &PgPool,
    service: &str,
    version: &'static str,
) -> (StatusCode, Json<Health>) {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .is_ok();

    let (status_code, status, database) = if db_ok {
        (StatusCode::OK, "ok", "up")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "degraded", "down")
    };

    (
        status_code,
        Json(Health {
            status,
            service: service.to_owned(),
            version,
            database,
        }),
    )
}
