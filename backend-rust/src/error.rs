use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{message}")]
    NotFound { message: String, logs: Vec<String> },
    #[error("{message}")]
    BadRequest { message: String, logs: Vec<String> },
    #[error("{message}")]
    Server { message: String, logs: Vec<String> },
}

#[derive(Serialize)]
struct ErrorDetail {
    message: String,
    logs: Vec<String>,
}

#[derive(Serialize)]
struct ErrorBody {
    detail: ErrorDetail,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message, logs) = match self {
            AppError::NotFound { message, logs } => (StatusCode::NOT_FOUND, message, logs),
            AppError::BadRequest { message, logs } => (StatusCode::BAD_REQUEST, message, logs),
            AppError::Server { message, logs } => (StatusCode::INTERNAL_SERVER_ERROR, message, logs),
        };
        let detail = ErrorDetail { message, logs };
        (status, Json(ErrorBody { detail })).into_response()
    }
}
