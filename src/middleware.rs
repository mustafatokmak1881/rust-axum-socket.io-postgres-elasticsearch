use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;

pub async fn request_logger(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();

    let response = next.run(request).await;

    tracing::info!(
        %method,
        %path,
        status = response.status().as_u16(),
        duration_ms = started.elapsed().as_millis() as u64,
        "HTTP request"
    );

    response
}
