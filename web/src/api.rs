//! REST API handlers

use actix_web::{web, HttpResponse, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::{AppState, ChartConfig};

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub uptime_seconds: u64,
}

#[derive(Deserialize)]
pub struct CandlesQuery {
    pub limit: Option<usize>,
    pub start: Option<u64>,
    pub end: Option<u64>,
}

#[derive(Deserialize)]
pub struct TradesQuery {
    pub limit: Option<usize>,
}

/// GET /api/v1/health
pub async fn health() -> Result<HttpResponse> {
    let response = HealthResponse {
        status: "ok",
        version: "0.1.0",
        uptime_seconds: 0, // TODO: track uptime
    };
    Ok(HttpResponse::Ok().json(response))
}

/// GET /api/v1/tickers
pub async fn get_tickers(
    state: web::Data<Arc<RwLock<AppState>>>,
) -> Result<HttpResponse> {
    let state = state.read();
    let tickers: Vec<_> = state.tickers.values().cloned().collect();
    Ok(HttpResponse::Ok().json(tickers))
}

/// GET /api/v1/tickers/{exchange}/{symbol}/candles
pub async fn get_candles(
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<(String, String)>,
    query: web::Query<CandlesQuery>,
) -> Result<HttpResponse> {
    let (exchange, symbol) = path.into_inner();
    let key = format!("{}:{}", exchange.to_lowercase(), symbol.to_uppercase());
    let limit = query.limit.unwrap_or(100).min(1000);
    
    let state = state.read();
    let candles = state.get_candles(&key, limit);
    
    Ok(HttpResponse::Ok().json(candles))
}

/// GET /api/v1/tickers/{exchange}/{symbol}/trades
pub async fn get_trades(
    state: web::Data<Arc<RwLock<AppState>>>,
    path: web::Path<(String, String)>,
    query: web::Query<TradesQuery>,
) -> Result<HttpResponse> {
    let (exchange, symbol) = path.into_inner();
    let key = format!("{}:{}", exchange.to_lowercase(), symbol.to_uppercase());
    let limit = query.limit.unwrap_or(100).min(1000);
    
    let state = state.read();
    let trades = state.get_trades(&key, limit);
    
    Ok(HttpResponse::Ok().json(trades))
}

/// GET /api/v1/config
pub async fn get_config(
    state: web::Data<Arc<RwLock<AppState>>>,
) -> Result<HttpResponse> {
    let state = state.read();
    Ok(HttpResponse::Ok().json(&state.config))
}

/// POST /api/v1/config
pub async fn save_config(
    state: web::Data<Arc<RwLock<AppState>>>,
    config: web::Json<ChartConfig>,
) -> Result<HttpResponse> {
    let mut state = state.write();
    state.config = config.into_inner();
    Ok(HttpResponse::Ok().json(&state.config))
}
