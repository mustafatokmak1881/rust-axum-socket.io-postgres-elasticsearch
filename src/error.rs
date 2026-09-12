use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::fmt;

pub enum AppError {
    Unauthorized,
    Forbidden,
    NotFound,
    BadRequest(&'static str),
    Internal(anyhow::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "unauthorized"),
            Self::Forbidden => write!(f, "forbidden"),
            Self::NotFound => write!(f, "not found"),
            Self::BadRequest(message) => write!(f, "{message}"),
            Self::Internal(error) => write!(f, "{error}"),
        }
    }
}

impl From<redis::RedisError> for AppError {
    fn from(error: redis::RedisError) -> Self {
        Self::Internal(error.into())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        Self::Internal(error.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "Authentication required or invalid authentication".to_owned(),
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Forbidden".to_owned()),
            Self::NotFound => (StatusCode::NOT_FOUND, "Resource not found".to_owned()),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, (*message).to_owned()),
            Self::Internal(error) => {
                tracing::error!(error = %error, "Internal request error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_owned(),
                )
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
