//! WebSocket handler for real-time data streaming

use actix_web::{web, HttpRequest, HttpResponse, Result};
use actix_ws::Message;
use parking_lot::RwLock;
use std::sync::Arc;
use futures::StreamExt;

use crate::state::AppState;
use crate::exchange::start_single_stream;

/// WebSocket handler for live data streaming
/// Route: /ws/live/{exchange}/{symbol}
pub async fn ws_handler(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse> {
    let (exchange, symbol) = path.into_inner();
    let exchange = exchange.to_lowercase();
    let symbol = symbol.to_uppercase();
    let key = format!("{}:{}", exchange, symbol);
    
    let (response, mut session, mut msg_stream) = actix_ws::handle(&req, stream)?;
    
    // Subscribe to updates for this symbol
    let mut rx = {
        let mut state_guard = state.write();
        state_guard.subscribe(&key)
    };
    
    log::info!("WebSocket client connected: {}", key);
    
    // Start a stream for this ticker if not already running
    let state_inner: Arc<RwLock<AppState>> = (**state).clone();
    let exchange_clone = exchange.clone();
    let symbol_clone = symbol.clone();
    let key_clone = key.clone();
    
    tokio::spawn(async move {
        start_single_stream(state_inner, &exchange_clone, &symbol_clone, &key_clone).await;
    });
    
    // Spawn task to handle the WebSocket connection
    actix_web::rt::spawn(async move {
        loop {
            tokio::select! {
                // Handle incoming messages from client
                Some(msg) = msg_stream.next() => {
                    match msg {
                        Ok(Message::Ping(bytes)) => {
                            if session.pong(&bytes).await.is_err() {
                                break;
                            }
                        }
                        Ok(Message::Text(text)) => {
                            // Handle client commands (e.g., subscribe to different symbol)
                            log::debug!("Received from client: {}", text);
                        }
                        Ok(Message::Close(_)) => {
                            log::info!("WebSocket closed by client: {}", key);
                            break;
                        }
                        Err(e) => {
                            log::error!("WebSocket error: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
                // Forward trade updates to client
                Some(trade_json) = rx.recv() => {
                    if session.text(trade_json).await.is_err() {
                        break;
                    }
                }
            }
        }
        
        let _ = session.close(None).await;
        log::info!("WebSocket disconnected: {}", key);
    });
    
    Ok(response)
}
