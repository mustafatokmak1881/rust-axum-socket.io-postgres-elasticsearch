use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use axum_extra::extract::CookieJar;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use super::protocol::{ClientMsg, ServerMsg};
use crate::{
    error::AppError,
    security::hash_token,
    state::SharedState,
    store::users,
};

pub async fn ws_upgrade(
    State(state): State<SharedState>,
    jar: CookieJar,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, AppError> {
    let session_token = jar
        .get(state.config.session_cookie_name())
        .ok_or(AppError::Unauthorized)?
        .value();

    let user = users::find_session_user(&state.redis, &hash_token(session_token))
        .await?
        .ok_or(AppError::Unauthorized)?;

    let user_id = user.id;
    Ok(ws.on_upgrade(move |socket| handle_socket(state, user_id, socket)))
}

async fn handle_socket(state: SharedState, user_id: uuid::Uuid, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();

    let conn_gen = state.hub.register(user_id, tx);

    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let Ok(text) = serde_json::to_string(&msg) else {
                continue;
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = stream.next().await {
        match message {
            Message::Text(text) => match serde_json::from_str::<ClientMsg>(&text) {
                Ok(msg) => state.hub.handle(user_id, msg).await,
                Err(_) => {
                    state.hub.send(
                        user_id,
                        ServerMsg::Error {
                            message: "Geçersiz mesaj".into(),
                        },
                    );
                }
            },
            Message::Ping(payload) => {
                let _ = payload;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    state.hub.unregister(user_id, conn_gen);
    send_task.abort();
}
