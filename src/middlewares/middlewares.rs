use axum::{extract::Request, middleware::Next, response::Response};

pub async fn logger_middleware(req: Request, next: Next) -> Response {
    println!("Url: {:?}", req.uri());
    let response = next.run(req).await;

    response
}

pub async fn is_authenticated(req: Request, next: Next) -> Response {
    if let Some(auth_header) = req.headers().get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            let clean_auth = auth_str
                .strip_prefix("Bearer")
                .or_else(|| auth_str.strip_prefix("bearer"))
                .unwrap_or(auth_str)
                .trim();
            println!("Authorization: {:?}", clean_auth);
        }
    }

    let response = next.run(req).await;

    response
}
