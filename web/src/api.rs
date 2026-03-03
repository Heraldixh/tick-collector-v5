//! REST API handlers

use actix_web::{web, HttpResponse, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::fs;
use std::path::Path;

use crate::state::{AppState, ChartConfig};

const FOOTPRINT_DATA_DIR: &str = "data/footprint";

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub uptime_seconds: u64,
}

#[derive(Serialize)]
pub struct DetailedHealthResponse {
    pub status: String,
    pub version: String,
    pub server_time: u64,
    pub uptime_seconds: u64,
    pub connections: ConnectionStats,
    pub storage: StorageStats,
    pub memory: MemoryStats,
    pub active_tickers: Vec<ActiveTickerInfo>,
}

#[derive(Serialize)]
pub struct ConnectionStats {
    pub total_tickers: usize,
    pub active_websocket_subscribers: usize,
    pub exchanges_connected: Vec<String>,
}

#[derive(Serialize)]
pub struct StorageStats {
    pub footprint_files: usize,
    pub total_size_bytes: u64,
    pub oldest_file_age_hours: f64,
    pub newest_file_age_hours: f64,
}

#[derive(Serialize)]
pub struct MemoryStats {
    pub trades_in_memory: usize,
    pub candles_in_memory: usize,
    pub symbols_tracked: usize,
}

#[derive(Serialize)]
pub struct ActiveTickerInfo {
    pub key: String,
    pub exchange: String,
    pub symbol: String,
    pub price: f64,
    pub trades_count: usize,
    pub subscribers: usize,
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

// Global start time for uptime tracking
use std::sync::OnceLock;
static START_TIME: OnceLock<std::time::Instant> = OnceLock::new();

pub fn init_start_time() {
    START_TIME.get_or_init(std::time::Instant::now);
}

/// GET /api/v1/health
pub async fn health() -> Result<HttpResponse> {
    let uptime = START_TIME.get()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    
    let response = HealthResponse {
        status: "ok",
        version: "0.1.0",
        uptime_seconds: uptime,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// GET /api/v1/health/detailed
pub async fn health_detailed(
    state: web::Data<Arc<RwLock<AppState>>>,
) -> Result<HttpResponse> {
    let state = state.read();
    
    let uptime = START_TIME.get()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    // Collect exchange names
    let mut exchanges: Vec<String> = state.tickers.values()
        .map(|t| t.exchange.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    exchanges.sort();
    
    // Count total subscribers
    let total_subscribers: usize = state.subscribers.values()
        .map(|v| v.len())
        .sum();
    
    // Get storage stats
    let storage = get_storage_stats();
    
    // Count trades and candles in memory
    let trades_in_memory: usize = state.trades.values()
        .map(|v| v.len())
        .sum();
    let candles_in_memory: usize = state.candles.values()
        .map(|v| v.len())
        .sum();
    
    // Build active tickers list
    let mut active_tickers: Vec<ActiveTickerInfo> = state.tickers.values()
        .map(|t| {
            let key = format!("{}:{}", t.exchange, t.symbol);
            let trades_count = state.trades.get(&key).map(|v| v.len()).unwrap_or(0);
            let subscribers = state.subscribers.get(&key).map(|v| v.len()).unwrap_or(0);
            ActiveTickerInfo {
                key: key.clone(),
                exchange: t.exchange.clone(),
                symbol: t.symbol.clone(),
                price: t.price,
                trades_count,
                subscribers,
            }
        })
        .collect();
    active_tickers.sort_by(|a, b| b.subscribers.cmp(&a.subscribers));
    
    let response = DetailedHealthResponse {
        status: "ok".to_string(),
        version: "0.1.0".to_string(),
        server_time: now,
        uptime_seconds: uptime,
        connections: ConnectionStats {
            total_tickers: state.tickers.len(),
            active_websocket_subscribers: total_subscribers,
            exchanges_connected: exchanges,
        },
        storage,
        memory: MemoryStats {
            trades_in_memory,
            candles_in_memory,
            symbols_tracked: state.trades.len(),
        },
        active_tickers,
    };
    
    Ok(HttpResponse::Ok().json(response))
}

fn get_storage_stats() -> StorageStats {
    let mut file_count = 0;
    let mut total_size: u64 = 0;
    let mut oldest_age_hours: f64 = 0.0;
    let mut newest_age_hours: f64 = f64::MAX;
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    if let Ok(entries) = fs::read_dir(FOOTPRINT_DATA_DIR) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                file_count += 1;
                total_size += metadata.len();
                
                // Try to read timestamp from file
                if let Ok(contents) = fs::read_to_string(entry.path()) {
                    if let Ok(data) = serde_json::from_str::<FootprintData>(&contents) {
                        let age_hours = (now - data.timestamp) as f64 / (1000.0 * 60.0 * 60.0);
                        if age_hours > oldest_age_hours {
                            oldest_age_hours = age_hours;
                        }
                        if age_hours < newest_age_hours {
                            newest_age_hours = age_hours;
                        }
                    }
                }
            }
        }
    }
    
    if newest_age_hours == f64::MAX {
        newest_age_hours = 0.0;
    }
    
    StorageStats {
        footprint_files: file_count,
        total_size_bytes: total_size,
        oldest_file_age_hours: oldest_age_hours,
        newest_file_age_hours: newest_age_hours,
    }
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

/// Footprint data structure for persistence
#[derive(Serialize, Deserialize, Clone)]
pub struct FootprintData {
    pub timestamp: u64,
    pub ticker_key: String,
    pub settings: FootprintSettings,
    pub bars: Vec<serde_json::Value>,
    pub all_trades: Vec<serde_json::Value>,
    pub tick_buffer: Vec<serde_json::Value>,
    pub last_price: Option<f64>,
    pub high_price: Option<f64>,
    pub low_price: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct FootprintSettings {
    pub tick_count: u32,
    pub tick_size_multiplier: u32,
    pub base_tick_size: f64,
    pub tick_size: f64,
}

/// GET /api/v1/footprint/{exchange}/{symbol}
pub async fn get_footprint(
    path: web::Path<(String, String)>,
) -> Result<HttpResponse> {
    let (exchange, symbol) = path.into_inner();
    let key = format!("{}_{}", exchange.to_lowercase(), symbol.to_uppercase());
    let file_path = format!("{}/{}.json", FOOTPRINT_DATA_DIR, key);
    
    // Check if file exists
    if !Path::new(&file_path).exists() {
        return Ok(HttpResponse::Ok().json(serde_json::json!(null)));
    }
    
    // Read and return file contents
    match fs::read_to_string(&file_path) {
        Ok(contents) => {
            match serde_json::from_str::<FootprintData>(&contents) {
                Ok(data) => {
                    // Check 7-day retention
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;
                    let age_days = (now - data.timestamp) as f64 / (1000.0 * 60.0 * 60.0 * 24.0);
                    
                    if age_days > 7.0 {
                        // Data too old, delete and return null
                        let _ = fs::remove_file(&file_path);
                        return Ok(HttpResponse::Ok().json(serde_json::json!(null)));
                    }
                    
                    Ok(HttpResponse::Ok().json(data))
                }
                Err(_) => Ok(HttpResponse::Ok().json(serde_json::json!(null))),
            }
        }
        Err(_) => Ok(HttpResponse::Ok().json(serde_json::json!(null))),
    }
}

/// POST /api/v1/footprint/{exchange}/{symbol}
pub async fn save_footprint(
    path: web::Path<(String, String)>,
    data: web::Json<FootprintData>,
) -> Result<HttpResponse> {
    let (exchange, symbol) = path.into_inner();
    let key = format!("{}_{}", exchange.to_lowercase(), symbol.to_uppercase());
    let file_path = format!("{}/{}.json", FOOTPRINT_DATA_DIR, key);
    
    // Ensure directory exists
    if let Err(e) = fs::create_dir_all(FOOTPRINT_DATA_DIR) {
        log::error!("Failed to create footprint data directory: {}", e);
        return Ok(HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Failed to create data directory"
        })));
    }
    
    // Write data to file
    match serde_json::to_string_pretty(&data.into_inner()) {
        Ok(json) => {
            match fs::write(&file_path, json) {
                Ok(_) => {
                    log::info!("Saved footprint data: {}", key);
                    Ok(HttpResponse::Ok().json(serde_json::json!({"status": "ok"})))
                }
                Err(e) => {
                    log::error!("Failed to write footprint data: {}", e);
                    Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                        "error": "Failed to write data"
                    })))
                }
            }
        }
        Err(e) => {
            log::error!("Failed to serialize footprint data: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to serialize data"
            })))
        }
    }
}

/// Cleanup old footprint data (called periodically)
pub fn cleanup_old_footprint_data() {
    let retention_days = 7;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let retention_ms = retention_days * 24 * 60 * 60 * 1000;
    
    if let Ok(entries) = fs::read_dir(FOOTPRINT_DATA_DIR) {
        for entry in entries.flatten() {
            if let Ok(contents) = fs::read_to_string(entry.path()) {
                if let Ok(data) = serde_json::from_str::<FootprintData>(&contents) {
                    if now - data.timestamp > retention_ms {
                        let _ = fs::remove_file(entry.path());
                        log::info!("Cleaned up old footprint data: {:?}", entry.path());
                    }
                }
            }
        }
    }
}
