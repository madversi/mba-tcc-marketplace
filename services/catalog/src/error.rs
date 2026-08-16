use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use domain::DomainError;
use serde::Serialize;

#[derive(Debug)]
pub enum ApiError {
    NotFound(&'static str),
    Validation(String),
    Conflict(String),
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
        Self::Validation(err.to_string())
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
