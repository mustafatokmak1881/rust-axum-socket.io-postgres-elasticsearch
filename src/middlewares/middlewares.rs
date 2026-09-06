use axum::{extract::Request, middleware::Next, response::Response};

pub async fn logger_middleware(req: Request, next: Next) -> Response {
    println!("Url: {:?}", req.uri());
    let response = next.run(req).await;

    response
}
