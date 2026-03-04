//! REST API handlers

use actix_web::{web, HttpRequest, HttpResponse, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::fs;
use std::path::Path;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::state::{AppState, ChartConfig};

const FOOTPRINT_DATA_DIR: &str = "data/footprint";
const TRADES_DATA_DIR: &str = "data/trades";
const API_KEY_FILE: &str = "data/api_key.txt";
const ADMIN_FILE: &str = "data/admin.json";
const SESSION_DURATION_HOURS: u64 = 24;
const PERSISTENCE_INTERVAL_SECS: u64 = 30;
const DATA_RETENTION_DAYS: u64 = 7;

// Admin credentials structure
#[derive(Serialize, Deserialize, Clone)]
pub struct AdminCredentials {
    pub username: String,
    pub password_hash: String,
    pub created_at: u64,
    pub updated_at: u64,
}

// Session token structure
#[derive(Serialize, Deserialize, Clone)]
pub struct SessionToken {
    pub token: String,
    pub username: String,
    pub created_at: u64,
    pub expires_at: u64,
}

// Active sessions storage
static SESSIONS: Lazy<Mutex<Vec<SessionToken>>> = Lazy::new(|| {
    Mutex::new(Vec::new())
});

/// Simple password hashing (SHA256)
fn hash_password(password: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    password.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Check if admin account exists
fn admin_exists() -> bool {
    if let Ok(contents) = fs::read_to_string(ADMIN_FILE) {
        if let Ok(_) = serde_json::from_str::<AdminCredentials>(&contents) {
            return true;
        }
    }
    false
}

/// Load admin credentials from file
fn load_admin() -> Option<AdminCredentials> {
    if let Ok(contents) = fs::read_to_string(ADMIN_FILE) {
        if let Ok(admin) = serde_json::from_str::<AdminCredentials>(&contents) {
            return Some(admin);
        }
    }
    None
}

/// Save admin credentials to file
fn save_admin(admin: &AdminCredentials) -> Result<(), std::io::Error> {
    let _ = fs::create_dir_all("data");
    let json = serde_json::to_string_pretty(admin).unwrap();
    fs::write(ADMIN_FILE, json)
}

/// Generate a session token
fn generate_session_token(username: &str) -> SessionToken {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let token = format!("sess_{}_{:x}", username, now);
    
    SessionToken {
        token,
        username: username.to_string(),
        created_at: now,
        expires_at: now + (SESSION_DURATION_HOURS * 3600),
    }
}

/// Validate session token from request
fn validate_session(req: &HttpRequest) -> bool {
    // Check cookie first
    if let Some(cookie) = req.cookie("session_token") {
        let token = cookie.value();
        return is_valid_session(token);
    }
    
    // Check Authorization header
    if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return is_valid_session(token);
            }
        }
    }
    
    false
}

/// Check if a session token is valid
fn is_valid_session(token: &str) -> bool {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let sessions = SESSIONS.lock();
    sessions.iter().any(|s| s.token == token && s.expires_at > now)
}

/// Clean up expired sessions
fn cleanup_expired_sessions() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let mut sessions = SESSIONS.lock();
    sessions.retain(|s| s.expires_at > now);
}

// API Key for desktop client authentication (mutable for regeneration)
static API_KEY: Lazy<Mutex<String>> = Lazy::new(|| {
    Mutex::new(load_or_generate_api_key())
});

/// Load API key from file or generate a new one
fn load_or_generate_api_key() -> String {
    // Try to load from file
    if let Ok(key) = fs::read_to_string(API_KEY_FILE) {
        let key = key.trim().to_string();
        if !key.is_empty() {
            log::info!("Loaded API key from file");
            return key;
        }
    }
    
    // Generate a new API key
    generate_new_api_key()
}

/// Generate a new API key and save to file
fn generate_new_api_key() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let key = format!("tc_{:x}", timestamp);
    
    // Save to file
    let _ = fs::create_dir_all("data");
    if fs::write(API_KEY_FILE, &key).is_ok() {
        log::info!("Generated and saved new API key: {}", &key[..10]);
    }
    
    key
}

/// Validate API key from request header
fn validate_api_key(req: &HttpRequest) -> bool {
    let current_key = API_KEY.lock().clone();
    
    if let Some(auth_header) = req.headers().get("X-API-Key") {
        if let Ok(provided_key) = auth_header.to_str() {
            return provided_key == current_key;
        }
    }
    // Also check query parameter for easier testing
    if let Some(query) = req.uri().query() {
        for param in query.split('&') {
            if let Some(key) = param.strip_prefix("api_key=") {
                return key == current_key;
            }
        }
    }
    false
}

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
/// Simple health check for load balancers and monitoring
pub async fn health() -> Result<HttpResponse> {
    let uptime = START_TIME.get()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    
    let response = HealthResponse {
        status: "ok",
        version: "1.0.0",
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

/// DELETE /api/v1/storage/clear-all
/// Clears all storage data (footprint and trades files)
/// Requires admin session authentication
pub async fn clear_all_storage(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> Result<HttpResponse> {
    // Validate admin session
    if !validate_session(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Admin authentication required"
        })));
    }
    
    let mut deleted_footprint = 0;
    let mut deleted_trades = 0;
    let mut errors: Vec<String> = Vec::new();
    
    // Clear footprint files
    if let Ok(entries) = fs::read_dir(FOOTPRINT_DATA_DIR) {
        for entry in entries.flatten() {
            if let Err(e) = fs::remove_file(entry.path()) {
                errors.push(format!("Failed to delete {:?}: {}", entry.path(), e));
            } else {
                deleted_footprint += 1;
            }
        }
    }
    
    // Clear trades files
    if let Ok(entries) = fs::read_dir(TRADES_DATA_DIR) {
        for entry in entries.flatten() {
            if let Err(e) = fs::remove_file(entry.path()) {
                errors.push(format!("Failed to delete {:?}: {}", entry.path(), e));
            } else {
                deleted_trades += 1;
            }
        }
    }
    
    // Clear in-memory trades
    {
        let mut state = state.write();
        state.trades.clear();
    }
    
    log::info!("🗑️ Storage cleared: {} footprint files, {} trades files deleted", deleted_footprint, deleted_trades);
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "deleted_footprint_files": deleted_footprint,
        "deleted_trades_files": deleted_trades,
        "errors": errors,
    })))
}

/// DELETE /api/v1/storage/clear/{exchange}/{symbol}
/// Clears storage data for a specific ticker
/// Requires admin session authentication
pub async fn clear_ticker_storage(
    req: HttpRequest,
    path: web::Path<(String, String)>,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> Result<HttpResponse> {
    // Validate admin session
    if !validate_session(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Admin authentication required"
        })));
    }
    
    let (exchange, symbol) = path.into_inner();
    let key = format!("{}:{}", exchange.to_lowercase(), symbol.to_uppercase());
    let file_prefix = format!("{}_{}", exchange.to_lowercase(), symbol.to_uppercase());
    
    let mut deleted_footprint = false;
    let mut deleted_trades = false;
    let mut errors: Vec<String> = Vec::new();
    
    // Delete footprint file
    let footprint_path = format!("{}/{}.json", FOOTPRINT_DATA_DIR, file_prefix);
    if Path::new(&footprint_path).exists() {
        if let Err(e) = fs::remove_file(&footprint_path) {
            errors.push(format!("Failed to delete footprint: {}", e));
        } else {
            deleted_footprint = true;
        }
    }
    
    // Delete trades file
    let trades_path = format!("{}/{}.json", TRADES_DATA_DIR, file_prefix);
    if Path::new(&trades_path).exists() {
        if let Err(e) = fs::remove_file(&trades_path) {
            errors.push(format!("Failed to delete trades: {}", e));
        } else {
            deleted_trades = true;
        }
    }
    
    // Clear in-memory trades for this ticker
    {
        let mut state = state.write();
        state.trades.remove(&key);
    }
    
    log::info!("🗑️ Storage cleared for {}: footprint={}, trades={}", key, deleted_footprint, deleted_trades);
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "ticker": key,
        "deleted_footprint": deleted_footprint,
        "deleted_trades": deleted_trades,
        "errors": errors,
    })))
}

/// GET /api/v1/storage/list
/// Lists all storage files with their sizes
/// Requires admin session authentication
pub async fn list_storage(
    req: HttpRequest,
) -> Result<HttpResponse> {
    // Validate admin session
    if !validate_session(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Admin authentication required"
        })));
    }
    
    let mut files: Vec<serde_json::Value> = Vec::new();
    
    // List footprint files
    if let Ok(entries) = fs::read_dir(FOOTPRINT_DATA_DIR) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                let filename = entry.file_name().to_string_lossy().to_string();
                // Extract exchange and symbol from filename (e.g., "binance_BTCUSDT.json")
                if let Some(name) = filename.strip_suffix(".json") {
                    let parts: Vec<&str> = name.splitn(2, '_').collect();
                    if parts.len() == 2 {
                        files.push(serde_json::json!({
                            "type": "footprint",
                            "exchange": parts[0],
                            "symbol": parts[1],
                            "key": format!("{}:{}", parts[0], parts[1]),
                            "size_bytes": metadata.len(),
                        }));
                    }
                }
            }
        }
    }
    
    // List trades files
    if let Ok(entries) = fs::read_dir(TRADES_DATA_DIR) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                let filename = entry.file_name().to_string_lossy().to_string();
                if let Some(name) = filename.strip_suffix(".json") {
                    let parts: Vec<&str> = name.splitn(2, '_').collect();
                    if parts.len() == 2 {
                        files.push(serde_json::json!({
                            "type": "trades",
                            "exchange": parts[0],
                            "symbol": parts[1],
                            "key": format!("{}:{}", parts[0], parts[1]),
                            "size_bytes": metadata.len(),
                        }));
                    }
                }
            }
        }
    }
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "files": files,
        "count": files.len(),
    })))
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
/// Saves admin's chart configuration and persists to disk
/// pane_tickers = what browser displays (can change freely)
/// active_tickers = what server collects 24/7 (persists independently of browser)
/// When admin adds a ticker to a pane, it's also added to active_tickers
/// active_tickers are NEVER removed by browser disconnect - only by explicit admin action
pub async fn save_config(
    state: web::Data<Arc<RwLock<AppState>>>,
    config: web::Json<ChartConfig>,
) -> Result<HttpResponse> {
    let mut config_data = config.into_inner();
    
    // Load existing config to preserve active_tickers
    let config_path = "data/config.json";
    let mut existing_active_tickers: Vec<String> = Vec::new();
    if let Ok(contents) = fs::read_to_string(config_path) {
        if let Ok(existing_config) = serde_json::from_str::<ChartConfig>(&contents) {
            existing_active_tickers = existing_config.active_tickers;
        }
    }
    
    // Add any new tickers from pane_tickers to active_tickers (but never remove)
    // This ensures tickers persist for 24/7 collection even when browser closes
    for ticker_opt in &config_data.pane_tickers {
        if let Some(ticker_key) = ticker_opt {
            if !existing_active_tickers.contains(ticker_key) {
                existing_active_tickers.push(ticker_key.clone());
                log::info!("➕ Added {} to active_tickers for 24/7 collection", ticker_key);
            }
        }
    }
    
    // Update config with merged active_tickers
    config_data.active_tickers = existing_active_tickers;
    
    // Save to memory
    {
        let mut state_guard = state.write();
        state_guard.config = config_data.clone();
    }
    
    // Persist to disk so server can restore on restart
    let _ = fs::create_dir_all("data");
    if let Ok(json) = serde_json::to_string_pretty(&config_data) {
        if let Err(e) = fs::write(config_path, &json) {
            log::error!("Failed to save config to disk: {}", e);
        } else {
            log::info!("📝 Config saved: panes={:?}, active_tickers={:?}", 
                config_data.pane_tickers, config_data.active_tickers);
        }
    }
    
    // Start 24/7 persistent streams for all active_tickers
    for ticker_key in &config_data.active_tickers {
        if let Some((exchange, symbol)) = ticker_key.split_once(':') {
            let state_inner: Arc<RwLock<AppState>> = (**state).clone();
            let key = ticker_key.clone();
            let exchange_owned = exchange.to_string();
            let symbol_owned = symbol.to_string();
            
            // Spawn persistent stream (will check if already running internally)
            tokio::spawn(async move {
                crate::exchange::start_single_stream(state_inner, &exchange_owned, &symbol_owned, &key).await;
            });
        }
    }
    
    Ok(HttpResponse::Ok().json(&config_data))
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

/// GET /api/v1/backup-trades/{exchange}/{symbol}
/// Fetch historical trades from exchange REST API (backup for WebSocket gaps)
pub async fn get_backup_trades(
    path: web::Path<(String, String)>,
    query: web::Query<TradesQuery>,
) -> Result<HttpResponse> {
    let (exchange, symbol) = path.into_inner();
    let limit = query.limit.unwrap_or(500).min(1000);
    
    log::info!("Backup trades request: {}:{} (limit={})", exchange, symbol, limit);
    
    match crate::exchange::fetch_historical_trades(&exchange.to_lowercase(), &symbol.to_uppercase(), limit).await {
        Ok(trades) => {
            log::info!("Returning {} backup trades for {}:{}", trades.len(), exchange, symbol);
            Ok(HttpResponse::Ok().json(trades))
        }
        Err(e) => {
            log::error!("Failed to fetch backup trades for {}:{}: {}", exchange, symbol, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to fetch trades: {}", e)
            })))
        }
    }
}

// ============================================================================
// SYNC API - Desktop Client Data Synchronization
// ============================================================================

#[derive(Serialize)]
pub struct SyncLatestResponse {
    pub exchange: String,
    pub symbol: String,
    pub latest_timestamp: u64,
    pub trades_count: usize,
    pub bars_count: usize,
    pub server_time: u64,
}

#[derive(Deserialize)]
pub struct SyncQuery {
    pub since: Option<u64>,
    pub limit: Option<usize>,
}

// ============================================================================
// AUTHENTICATION API
// ============================================================================

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct UpdateCredentialsRequest {
    pub current_password: String,
    pub new_username: Option<String>,
    pub new_password: Option<String>,
}

/// GET /api/v1/auth/status
/// Check if admin account exists and if user is logged in
pub async fn auth_status(req: HttpRequest) -> Result<HttpResponse> {
    let admin_configured = admin_exists();
    let is_authenticated = validate_session(&req);
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "admin_configured": admin_configured,
        "is_authenticated": is_authenticated,
        "session_duration_hours": SESSION_DURATION_HOURS
    })))
}

/// POST /api/v1/auth/setup
/// Initial admin account setup (only works if no admin exists)
pub async fn auth_setup(body: web::Json<SetupRequest>) -> Result<HttpResponse> {
    // Check if admin already exists
    if admin_exists() {
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Admin account already configured. Use login instead."
        })));
    }
    
    // Validate input
    if body.username.trim().is_empty() || body.password.len() < 4 {
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Username cannot be empty and password must be at least 4 characters"
        })));
    }
    
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let admin = AdminCredentials {
        username: body.username.trim().to_string(),
        password_hash: hash_password(&body.password),
        created_at: now,
        updated_at: now,
    };
    
    if let Err(e) = save_admin(&admin) {
        log::error!("Failed to save admin credentials: {}", e);
        return Ok(HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Failed to save admin credentials"
        })));
    }
    
    log::info!("Admin account created for user: {}", admin.username);
    
    // Auto-login after setup
    let session = generate_session_token(&admin.username);
    {
        let mut sessions = SESSIONS.lock();
        sessions.push(session.clone());
    }
    
    Ok(HttpResponse::Ok()
        .cookie(
            actix_web::cookie::Cookie::build("session_token", &session.token)
                .path("/")
                .http_only(true)
                .max_age(actix_web::cookie::time::Duration::hours(SESSION_DURATION_HOURS as i64))
                .finish()
        )
        .json(serde_json::json!({
            "success": true,
            "message": "Admin account created successfully",
            "username": admin.username,
            "token": session.token
        })))
}

/// POST /api/v1/auth/login
/// Login with username and password
pub async fn auth_login(body: web::Json<LoginRequest>) -> Result<HttpResponse> {
    cleanup_expired_sessions();
    
    let admin = match load_admin() {
        Some(a) => a,
        None => {
            return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                "error": "No admin account configured. Please run setup first."
            })));
        }
    };
    
    // Verify credentials
    let password_hash = hash_password(&body.password);
    if body.username != admin.username || password_hash != admin.password_hash {
        log::warn!("Failed login attempt for user: {}", body.username);
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Invalid username or password"
        })));
    }
    
    // Create session
    let session = generate_session_token(&admin.username);
    {
        let mut sessions = SESSIONS.lock();
        sessions.push(session.clone());
    }
    
    log::info!("User logged in: {}", admin.username);
    
    Ok(HttpResponse::Ok()
        .cookie(
            actix_web::cookie::Cookie::build("session_token", &session.token)
                .path("/")
                .http_only(true)
                .max_age(actix_web::cookie::time::Duration::hours(SESSION_DURATION_HOURS as i64))
                .finish()
        )
        .json(serde_json::json!({
            "success": true,
            "message": "Login successful",
            "username": admin.username,
            "token": session.token
        })))
}

/// POST /api/v1/auth/logout
/// Logout and invalidate session
pub async fn auth_logout(req: HttpRequest) -> Result<HttpResponse> {
    // Get token from cookie or header
    let token = if let Some(cookie) = req.cookie("session_token") {
        Some(cookie.value().to_string())
    } else if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            auth_str.strip_prefix("Bearer ").map(|s| s.to_string())
        } else {
            None
        }
    } else {
        None
    };
    
    // Remove session
    if let Some(token) = token {
        let mut sessions = SESSIONS.lock();
        sessions.retain(|s| s.token != token);
    }
    
    Ok(HttpResponse::Ok()
        .cookie(
            actix_web::cookie::Cookie::build("session_token", "")
                .path("/")
                .http_only(true)
                .max_age(actix_web::cookie::time::Duration::seconds(0))
                .finish()
        )
        .json(serde_json::json!({
            "success": true,
            "message": "Logged out successfully"
        })))
}

/// POST /api/v1/auth/update
/// Update admin credentials (requires authentication)
pub async fn auth_update(req: HttpRequest, body: web::Json<UpdateCredentialsRequest>) -> Result<HttpResponse> {
    // Verify session
    if !validate_session(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Not authenticated"
        })));
    }
    
    let mut admin = match load_admin() {
        Some(a) => a,
        None => {
            return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                "error": "No admin account found"
            })));
        }
    };
    
    // Verify current password
    let current_hash = hash_password(&body.current_password);
    if current_hash != admin.password_hash {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Current password is incorrect"
        })));
    }
    
    // Update username if provided
    if let Some(ref new_username) = body.new_username {
        if new_username.trim().is_empty() {
            return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Username cannot be empty"
            })));
        }
        admin.username = new_username.trim().to_string();
    }
    
    // Update password if provided
    if let Some(ref new_password) = body.new_password {
        if new_password.len() < 4 {
            return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Password must be at least 4 characters"
            })));
        }
        admin.password_hash = hash_password(new_password);
    }
    
    use std::time::{SystemTime, UNIX_EPOCH};
    admin.updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    if let Err(e) = save_admin(&admin) {
        log::error!("Failed to update admin credentials: {}", e);
        return Ok(HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Failed to save credentials"
        })));
    }
    
    log::info!("Admin credentials updated for user: {}", admin.username);
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": "Credentials updated successfully",
        "username": admin.username
    })))
}

/// GET /api/v1/auth/check
/// Check if current session is valid (for protected routes)
pub async fn auth_check(req: HttpRequest) -> Result<HttpResponse> {
    if !validate_session(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "authenticated": false,
            "error": "Not authenticated"
        })));
    }
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "authenticated": true
    })))
}

/// GET /api/v1/api-key
/// Get the API key for desktop client configuration (only accessible from web app)
pub async fn get_api_key() -> Result<HttpResponse> {
    let current_key = API_KEY.lock().clone();
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "api_key": current_key,
        "header_name": "X-API-Key",
        "usage": "Add header 'X-API-Key: <api_key>' to all sync requests"
    })))
}

/// POST /api/v1/api-key/regenerate
/// Regenerate the API key (invalidates old key immediately)
pub async fn regenerate_api_key() -> Result<HttpResponse> {
    // Generate new key
    let new_key = generate_new_api_key();
    
    // Update the in-memory key
    {
        let mut key = API_KEY.lock();
        *key = new_key.clone();
    }
    
    log::info!("API key regenerated by user request");
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": "API key regenerated successfully. Old key is now invalid.",
        "api_key": new_key,
        "header_name": "X-API-Key"
    })))
}

/// GET /api/v1/sync/{exchange}/{symbol}/latest
/// Get the latest trade timestamp and counts for sync coordination
/// Requires API Key authentication
pub async fn sync_latest(
    req: HttpRequest,
    path: web::Path<(String, String)>,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> Result<HttpResponse> {
    // Validate API key
    if !validate_api_key(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Invalid or missing API key",
            "hint": "Add header 'X-API-Key: <your_api_key>' or query param '?api_key=<your_api_key>'"
        })));
    }
    
    let (exchange, symbol) = path.into_inner();
    let key = format!("{}:{}", exchange.to_lowercase(), symbol.to_uppercase());
    
    let state = state.read();
    
    let (latest_timestamp, trades_count) = state.trades.get(&key)
        .map(|trades| {
            let latest = trades.back().map(|t| t.timestamp).unwrap_or(0);
            (latest, trades.len())
        })
        .unwrap_or((0, 0));
    
    // Also check footprint data file for historical data
    let file_path = format!("{}/{}_{}.json", FOOTPRINT_DATA_DIR, exchange.to_lowercase(), symbol.to_uppercase());
    let (file_latest, bars_count) = if let Ok(contents) = fs::read_to_string(&file_path) {
        if let Ok(data) = serde_json::from_str::<FootprintData>(&contents) {
            let file_ts = data.all_trades.last()
                .and_then(|t| t.get("timestamp"))
                .and_then(|v: &serde_json::Value| v.as_u64())
                .unwrap_or(data.timestamp);
            let bars = data.bars.len();
            (file_ts, bars)
        } else {
            (0, 0)
        }
    } else {
        (0, 0)
    };
    
    let final_latest = latest_timestamp.max(file_latest);
    
    let response = SyncLatestResponse {
        exchange: exchange.to_lowercase(),
        symbol: symbol.to_uppercase(),
        latest_timestamp: final_latest,
        trades_count,
        bars_count,
        server_time: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    };
    
    Ok(HttpResponse::Ok().json(response))
}

/// GET /api/v1/sync/{exchange}/{symbol}/trades
/// Get historical trades for desktop client sync
/// Requires API Key authentication
pub async fn sync_trades(
    req: HttpRequest,
    path: web::Path<(String, String)>,
    query: web::Query<SyncQuery>,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> Result<HttpResponse> {
    // Validate API key
    if !validate_api_key(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Invalid or missing API key",
            "hint": "Add header 'X-API-Key: <your_api_key>' or query param '?api_key=<your_api_key>'"
        })));
    }
    
    let (exchange, symbol) = path.into_inner();
    let key = format!("{}:{}", exchange.to_lowercase(), symbol.to_uppercase());
    let since = query.since.unwrap_or(0);
    let limit = query.limit.unwrap_or(10000).min(50000);
    
    log::info!("Sync trades request: {} since={} limit={}", key, since, limit);
    
    let mut all_trades: Vec<serde_json::Value> = Vec::new();
    
    // 1. First, try to get trades from server-side trades data file (background persistence)
    let trades_file_path = format!("{}/{}_{}.json", TRADES_DATA_DIR, exchange.to_lowercase(), symbol.to_uppercase());
    if let Ok(contents) = fs::read_to_string(&trades_file_path) {
        if let Ok(data) = serde_json::from_str::<TradesFileData>(&contents) {
            for trade in data.trades {
                let ts = trade.get("timestamp").and_then(|v: &serde_json::Value| v.as_u64()).unwrap_or(0);
                if ts > since {
                    all_trades.push(trade);
                }
            }
            log::debug!("Loaded {} trades from server trades file", all_trades.len());
        }
    }
    
    // 2. Also try footprint data file (browser-saved data)
    let footprint_file_path = format!("{}/{}_{}.json", FOOTPRINT_DATA_DIR, exchange.to_lowercase(), symbol.to_uppercase());
    if let Ok(contents) = fs::read_to_string(&footprint_file_path) {
        if let Ok(data) = serde_json::from_str::<FootprintData>(&contents) {
            for trade in data.all_trades {
                let ts = trade.get("timestamp").and_then(|v: &serde_json::Value| v.as_u64()).unwrap_or(0);
                if ts > since {
                    all_trades.push(trade);
                }
            }
        }
    }
    
    // 3. Also add in-memory trades (most recent)
    {
        let state = state.read();
        if let Some(trades) = state.trades.get(&key) {
            for trade in trades.iter() {
                if trade.timestamp > since {
                    all_trades.push(serde_json::json!({
                        "timestamp": trade.timestamp,
                        "price": trade.price,
                        "quantity": trade.quantity,
                        "is_buyer_maker": trade.is_buyer_maker,
                    }));
                }
            }
        }
    }
    
    // Sort by timestamp
    all_trades.sort_by(|a, b| {
        let ts_a = a.get("timestamp").and_then(|v: &serde_json::Value| v.as_u64()).unwrap_or(0);
        let ts_b = b.get("timestamp").and_then(|v: &serde_json::Value| v.as_u64()).unwrap_or(0);
        ts_a.cmp(&ts_b)
    });
    
    // Deduplicate by timestamp + price + quantity (to avoid losing trades at same ms)
    all_trades.dedup_by(|a, b| {
        let ts_a = a.get("timestamp").and_then(|v: &serde_json::Value| v.as_u64()).unwrap_or(0);
        let ts_b = b.get("timestamp").and_then(|v: &serde_json::Value| v.as_u64()).unwrap_or(0);
        let price_a = a.get("price").and_then(|v: &serde_json::Value| v.as_f64()).unwrap_or(0.0);
        let price_b = b.get("price").and_then(|v: &serde_json::Value| v.as_f64()).unwrap_or(0.0);
        let qty_a = a.get("quantity").and_then(|v: &serde_json::Value| v.as_f64()).unwrap_or(0.0);
        let qty_b = b.get("quantity").and_then(|v: &serde_json::Value| v.as_f64()).unwrap_or(0.0);
        ts_a == ts_b && (price_a - price_b).abs() < 0.0000001 && (qty_a - qty_b).abs() < 0.0000001
    });
    
    // Limit results
    if all_trades.len() > limit {
        all_trades = all_trades.into_iter().take(limit).collect();
    }
    
    log::info!("Returning {} trades for sync: {}", all_trades.len(), key);
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "exchange": exchange.to_lowercase(),
        "symbol": symbol.to_uppercase(),
        "since": since,
        "count": all_trades.len(),
        "trades": all_trades,
    })))
}

/// GET /api/v1/sync/{exchange}/{symbol}/bars
/// Get pre-aggregated footprint bars for desktop client sync
/// Requires API Key authentication
pub async fn sync_bars(
    req: HttpRequest,
    path: web::Path<(String, String)>,
    query: web::Query<SyncQuery>,
) -> Result<HttpResponse> {
    // Validate API key
    if !validate_api_key(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Invalid or missing API key",
            "hint": "Add header 'X-API-Key: <your_api_key>' or query param '?api_key=<your_api_key>'"
        })));
    }
    
    let (exchange, symbol) = path.into_inner();
    let since = query.since.unwrap_or(0);
    let limit = query.limit.unwrap_or(200).min(500);
    
    log::info!("Sync bars request: {}:{} since={} limit={}", exchange, symbol, since, limit);
    
    let file_path = format!("{}/{}_{}.json", FOOTPRINT_DATA_DIR, exchange.to_lowercase(), symbol.to_uppercase());
    
    if let Ok(contents) = fs::read_to_string(&file_path) {
        if let Ok(data) = serde_json::from_str::<FootprintData>(&contents) {
            let bars: Vec<serde_json::Value> = data.bars
                .into_iter()
                .filter(|bar: &serde_json::Value| {
                    let ts = bar.get("time").and_then(|v: &serde_json::Value| v.as_u64()).unwrap_or(0);
                    ts > since
                })
                .take(limit)
                .collect();
            
            log::info!("Returning {} bars for sync: {}:{}", bars.len(), exchange, symbol);
            
            return Ok(HttpResponse::Ok().json(serde_json::json!({
                "exchange": exchange.to_lowercase(),
                "symbol": symbol.to_uppercase(),
                "since": since,
                "count": bars.len(),
                "tick_count": data.settings.tick_count,
                "tick_size": data.settings.tick_size,
                "bars": bars,
            })));
        }
    }
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "exchange": exchange.to_lowercase(),
        "symbol": symbol.to_uppercase(),
        "since": since,
        "count": 0,
        "bars": [],
    })))
}

/// GET /api/v1/sync/tickers
/// Get list of all available tickers with sync status
/// Requires API Key authentication
pub async fn sync_tickers(
    req: HttpRequest,
    state: web::Data<Arc<RwLock<AppState>>>,
) -> Result<HttpResponse> {
    // Validate API key
    if !validate_api_key(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Invalid or missing API key",
            "hint": "Add header 'X-API-Key: <your_api_key>' or query param '?api_key=<your_api_key>'"
        })));
    }
    
    let state = state.read();
    
    let mut tickers: Vec<serde_json::Value> = Vec::new();
    
    for (key, info) in state.tickers.iter() {
        let trades_count = state.trades.get(key).map(|t| t.len()).unwrap_or(0);
        let subscribers = state.subscribers.get(key).map(|s| s.len()).unwrap_or(0);
        
        // Check for footprint data file
        let parts: Vec<&str> = key.split(':').collect();
        let (exchange, symbol) = if parts.len() == 2 { (parts[0], parts[1]) } else { continue };
        let file_path = format!("{}/{}_{}.json", FOOTPRINT_DATA_DIR, exchange, symbol);
        let has_data = Path::new(&file_path).exists();
        
        tickers.push(serde_json::json!({
            "key": key,
            "exchange": info.exchange,
            "symbol": info.symbol,
            "price": info.price,
            "trades_in_memory": trades_count,
            "active_subscribers": subscribers,
            "has_historical_data": has_data,
        }));
    }
    
    // Sort by subscribers (active first)
    tickers.sort_by(|a, b| {
        let subs_a = a.get("active_subscribers").and_then(|v| v.as_u64()).unwrap_or(0);
        let subs_b = b.get("active_subscribers").and_then(|v| v.as_u64()).unwrap_or(0);
        subs_b.cmp(&subs_a)
    });
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "count": tickers.len(),
        "tickers": tickers,
    })))
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

// ============================================================================
// BACKGROUND DATA PERSISTENCE
// ============================================================================

/// Structure for persisting trades to files
#[derive(Serialize, Deserialize)]
struct TradesFileData {
    exchange: String,
    symbol: String,
    last_updated: u64,
    trades: Vec<serde_json::Value>,
}

/// Start background persistence task that saves trades to files continuously
/// This ensures data is collected even when no browser is connected
pub async fn start_background_persistence(state: Arc<RwLock<AppState>>) {
    log::info!("📦 Starting background data persistence (every {}s, {}-day retention)", 
        PERSISTENCE_INTERVAL_SECS, DATA_RETENTION_DAYS);
    
    // Create data directories
    let _ = fs::create_dir_all(TRADES_DATA_DIR);
    let _ = fs::create_dir_all(FOOTPRINT_DATA_DIR);
    
    let mut cleanup_counter = 0u64;
    
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(PERSISTENCE_INTERVAL_SECS)).await;
        
        // Get all trades from state and save to files
        let trades_to_save: Vec<(String, Vec<crate::state::Trade>)> = {
            let state_guard = state.read();
            state_guard.trades.iter()
                .filter(|(_, trades)| !trades.is_empty())
                .map(|(key, trades)| (key.clone(), trades.iter().cloned().collect()))
                .collect()
        };
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        let mut saved_count = 0;
        
        for (key, trades) in trades_to_save {
            if let Some((exchange, symbol)) = key.split_once(':') {
                let file_path = format!("{}/{}_{}.json", TRADES_DATA_DIR, exchange, symbol);
                
                // Load existing trades from file
                let mut all_trades: Vec<serde_json::Value> = if let Ok(contents) = fs::read_to_string(&file_path) {
                    if let Ok(data) = serde_json::from_str::<TradesFileData>(&contents) {
                        data.trades
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                
                // Add new trades
                for trade in &trades {
                    all_trades.push(serde_json::json!({
                        "timestamp": trade.timestamp,
                        "price": trade.price,
                        "quantity": trade.quantity,
                        "is_buyer_maker": trade.is_buyer_maker,
                    }));
                }
                
                // Sort by timestamp
                all_trades.sort_by(|a, b| {
                    let ts_a = a.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
                    let ts_b = b.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
                    ts_a.cmp(&ts_b)
                });
                
                // Deduplicate by timestamp + price + quantity
                all_trades.dedup_by(|a, b| {
                    let ts_a = a.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
                    let ts_b = b.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
                    let price_a = a.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let price_b = b.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let qty_a = a.get("quantity").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let qty_b = b.get("quantity").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    ts_a == ts_b && (price_a - price_b).abs() < 0.0000001 && (qty_a - qty_b).abs() < 0.0000001
                });
                
                // Apply 7-day retention - remove trades older than retention period
                let retention_ms = DATA_RETENTION_DAYS * 24 * 60 * 60 * 1000;
                let cutoff = now.saturating_sub(retention_ms);
                all_trades.retain(|t| {
                    t.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0) > cutoff
                });
                
                // Save to file
                let data = TradesFileData {
                    exchange: exchange.to_string(),
                    symbol: symbol.to_string(),
                    last_updated: now,
                    trades: all_trades,
                };
                
                if let Ok(json) = serde_json::to_string(&data) {
                    if fs::write(&file_path, json).is_ok() {
                        saved_count += 1;
                    }
                }
            }
        }
        
        if saved_count > 0 {
            log::info!("💾 Persisted trades for {} tickers to disk", saved_count);
        }
        
        // Run cleanup every 10 cycles (5 minutes)
        cleanup_counter += 1;
        if cleanup_counter % 10 == 0 {
            cleanup_old_trades_data();
            cleanup_old_footprint_data();
        }
    }
}

/// Cleanup old trades data files (7-day retention)
fn cleanup_old_trades_data() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let retention_ms = DATA_RETENTION_DAYS * 24 * 60 * 60 * 1000;
    
    if let Ok(entries) = fs::read_dir(TRADES_DATA_DIR) {
        for entry in entries.flatten() {
            if let Ok(contents) = fs::read_to_string(entry.path()) {
                if let Ok(data) = serde_json::from_str::<TradesFileData>(&contents) {
                    // If file hasn't been updated in retention period, delete it
                    if now - data.last_updated > retention_ms {
                        let _ = fs::remove_file(entry.path());
                        log::info!("🗑️ Cleaned up old trades data: {:?}", entry.path());
                    }
                }
            }
        }
    }
    
    // Also cleanup SQLite database
    let _ = crate::db::cleanup_old_data(DATA_RETENTION_DAYS);
}

/// GET /api/v1/db/stats
/// Get database statistics
pub async fn db_stats() -> Result<HttpResponse> {
    match crate::db::get_stats() {
        Ok(stats) => Ok(HttpResponse::Ok().json(stats)),
        Err(e) => Ok(HttpResponse::InternalServerError().json(serde_json::json!({
            "error": format!("Failed to get database stats: {}", e)
        }))),
    }
}

/// GET /api/v1/active-tickers
/// Get list of tickers being collected 24/7 (independent of browser state)
pub async fn get_active_tickers() -> Result<HttpResponse> {
    let config_path = "data/config.json";
    if let Ok(contents) = fs::read_to_string(config_path) {
        if let Ok(config) = serde_json::from_str::<ChartConfig>(&contents) {
            return Ok(HttpResponse::Ok().json(serde_json::json!({
                "active_tickers": config.active_tickers,
                "count": config.active_tickers.len()
            })));
        }
    }
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "active_tickers": Vec::<String>::new(),
        "count": 0
    })))
}

/// DELETE /api/v1/active-tickers/{exchange}/{symbol}
/// Remove a ticker from 24/7 collection (admin only - stops the stream)
pub async fn remove_active_ticker(
    path: web::Path<(String, String)>,
) -> Result<HttpResponse> {
    let (exchange, symbol) = path.into_inner();
    let ticker_key = format!("{}:{}", exchange.to_lowercase(), symbol.to_uppercase());
    
    let config_path = "data/config.json";
    
    // Load existing config
    let mut config = if let Ok(contents) = fs::read_to_string(config_path) {
        serde_json::from_str::<ChartConfig>(&contents).unwrap_or_default()
    } else {
        ChartConfig::default()
    };
    
    // Remove from active_tickers
    let original_len = config.active_tickers.len();
    config.active_tickers.retain(|t| t != &ticker_key);
    
    if config.active_tickers.len() == original_len {
        return Ok(HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Ticker {} not found in active_tickers", ticker_key)
        })));
    }
    
    // Also remove from pane_tickers if present
    for pane in config.pane_tickers.iter_mut() {
        if pane.as_ref() == Some(&ticker_key) {
            *pane = None;
        }
    }
    
    // Save updated config
    if let Ok(json) = serde_json::to_string_pretty(&config) {
        if let Err(e) = fs::write(config_path, &json) {
            log::error!("Failed to save config: {}", e);
            return Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to save config: {}", e)
            })));
        }
    }
    
    log::info!("🗑️ Removed {} from active_tickers - stream will stop", ticker_key);
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "removed": ticker_key,
        "remaining_active_tickers": config.active_tickers
    })))
}
