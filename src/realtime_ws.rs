#![allow(dead_code)]
//! WebSocket handler for realtime change notifications.

use crate::auth;
use crate::config::AppConfig;
use crate::realtime::{ClientMessage, RealtimeEngine, ServerMessage};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Combined state for the websocket handler.
#[derive(Clone)]
pub struct WsState {
    pub engine: Arc<RealtimeEngine>,
    pub config: AppConfig,
}

#[derive(serde::Deserialize)]
pub struct WsQuery {
    #[serde(default)]
    token: Option<String>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<WsState>,
    Query(query): Query<WsQuery>,
) -> Response {
    let claims = if let Some(ref token) = query.token {
        let header = format!("Bearer {}", token);
        match auth::authenticate(Some(&header), &state.config) {
            Ok(c) => c,
            Err(_) => None,
        }
    } else {
        None
    };

    ws.on_upgrade(move |socket| handle_socket(socket, state.engine, claims))
}

async fn handle_socket(
    socket: WebSocket,
    engine: Arc<RealtimeEngine>,
    _claims: Option<auth::Claims>,
) {
    let client_id = Uuid::new_v4();
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(256);

    // Forward engine messages to websocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if ws_tx.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Read client messages
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                    match client_msg {
                        ClientMessage::Subscribe {
                            id,
                            table,
                            filter,
                            events,
                        } => match engine
                            .subscribe(
                                client_id,
                                id.clone(),
                                &table,
                                filter.as_deref(),
                                events,
                                tx.clone(),
                            )
                            .await
                        {
                            Ok(table_key) => {
                                let _ = tx
                                    .send(ServerMessage::Subscribed {
                                        type_: "subscribed",
                                        id,
                                        table: table_key,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(ServerMessage::Error {
                                        type_: "error",
                                        message: e,
                                    })
                                    .await;
                            }
                        },
                        ClientMessage::Unsubscribe { id } => {
                            engine.unsubscribe(client_id, &id).await;
                            let _ = tx
                                .send(ServerMessage::Unsubscribed {
                                    type_: "unsubscribed",
                                    id,
                                })
                                .await;
                        }
                        ClientMessage::Ping => {
                            let _ = tx.send(ServerMessage::Pong { type_: "pong" }).await;
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    engine.remove_client(client_id).await;
    send_task.abort();
}
