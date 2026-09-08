use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub enum AppError {
    Unauthorized,
    Forbidden,
    NotFound,
    BadRequest(&'static str),
    Internal(anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        Self::Internal(error.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "Authentication required or invalid authentication",
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "Resource not found"),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::Internal(error) => {
                tracing::error!(error = %error, "Internal request error");

                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
