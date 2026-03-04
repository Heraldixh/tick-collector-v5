//! SQLite database module for persistent trade storage
//! 
//! Provides lightweight persistence (~5MB RAM) with SQL query support.
//! Designed for multi-client access and 7-day data retention.

use rusqlite::{Connection, params};
use std::sync::Arc;
use parking_lot::Mutex;
use once_cell::sync::Lazy;

const DB_PATH: &str = "data/tick_collector.db";

/// Global database connection (thread-safe)
pub static DB: Lazy<Arc<Mutex<Connection>>> = Lazy::new(|| {
    let conn = init_database().expect("Failed to initialize database");
    Arc::new(Mutex::new(conn))
});

/// Initialize the SQLite database with required tables
fn init_database() -> Result<Connection, rusqlite::Error> {
    std::fs::create_dir_all("data").ok();
    
    let conn = Connection::open(DB_PATH)?;
    
    // Enable WAL mode for better concurrent read performance
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    
    // Create trades table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS trades (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            exchange TEXT NOT NULL,
            symbol TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            price REAL NOT NULL,
            quantity REAL NOT NULL,
            is_buyer_maker INTEGER NOT NULL,
            created_at INTEGER DEFAULT (strftime('%s', 'now') * 1000)
        )",
        [],
    )?;
    
    // Create index for fast queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_trades_ticker_time 
         ON trades (exchange, symbol, timestamp DESC)",
        [],
    )?;
    
    // Create config table for ticker persistence
    conn.execute(
        "CREATE TABLE IF NOT EXISTS config (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER DEFAULT (strftime('%s', 'now') * 1000)
        )",
        [],
    )?;
    
    // Create footprint bars table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS footprint_bars (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            exchange TEXT NOT NULL,
            symbol TEXT NOT NULL,
            bar_time INTEGER NOT NULL,
            bar_data TEXT NOT NULL,
            created_at INTEGER DEFAULT (strftime('%s', 'now') * 1000),
            UNIQUE(exchange, symbol, bar_time)
        )",
        [],
    )?;
    
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_footprint_ticker_time 
         ON footprint_bars (exchange, symbol, bar_time DESC)",
        [],
    )?;
    
    log::info!("📦 SQLite database initialized: {}", DB_PATH);
    
    Ok(conn)
}

/// Insert a trade into the database
pub fn insert_trade(
    exchange: &str,
    symbol: &str,
    timestamp: u64,
    price: f64,
    quantity: f64,
    is_buyer_maker: bool,
) -> Result<(), rusqlite::Error> {
    let conn = DB.lock();
    conn.execute(
        "INSERT INTO trades (exchange, symbol, timestamp, price, quantity, is_buyer_maker)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![exchange, symbol, timestamp as i64, price, quantity, is_buyer_maker as i32],
    )?;
    Ok(())
}

/// Insert multiple trades in a batch (more efficient)
pub fn insert_trades_batch(
    trades: &[(String, String, u64, f64, f64, bool)],
) -> Result<usize, rusqlite::Error> {
    let mut conn = DB.lock();
    let tx = conn.transaction()?;
    
    let mut count = 0;
    for (exchange, symbol, timestamp, price, quantity, is_buyer_maker) in trades {
        tx.execute(
            "INSERT OR IGNORE INTO trades (exchange, symbol, timestamp, price, quantity, is_buyer_maker)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![exchange, symbol, *timestamp as i64, price, quantity, *is_buyer_maker as i32],
        )?;
        count += 1;
    }
    
    tx.commit()?;
    Ok(count)
}

/// Get trades for a ticker since a timestamp
pub fn get_trades(
    exchange: &str,
    symbol: &str,
    since: u64,
    limit: usize,
) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
    let conn = DB.lock();
    let mut stmt = conn.prepare(
        "SELECT timestamp, price, quantity, is_buyer_maker 
         FROM trades 
         WHERE exchange = ?1 AND symbol = ?2 AND timestamp > ?3
         ORDER BY timestamp ASC
         LIMIT ?4"
    )?;
    
    let trades = stmt.query_map(
        params![exchange, symbol, since as i64, limit as i64],
        |row| {
            Ok(serde_json::json!({
                "timestamp": row.get::<_, i64>(0)? as u64,
                "price": row.get::<_, f64>(1)?,
                "quantity": row.get::<_, f64>(2)?,
                "is_buyer_maker": row.get::<_, i32>(3)? != 0,
            }))
        },
    )?.filter_map(|r| r.ok()).collect();
    
    Ok(trades)
}

/// Get latest trade timestamp for a ticker
pub fn get_latest_timestamp(exchange: &str, symbol: &str) -> Result<u64, rusqlite::Error> {
    let conn = DB.lock();
    let result: Result<i64, _> = conn.query_row(
        "SELECT MAX(timestamp) FROM trades WHERE exchange = ?1 AND symbol = ?2",
        params![exchange, symbol],
        |row| row.get(0),
    );
    
    Ok(result.unwrap_or(0) as u64)
}

/// Get trade count for a ticker
pub fn get_trade_count(exchange: &str, symbol: &str) -> Result<usize, rusqlite::Error> {
    let conn = DB.lock();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM trades WHERE exchange = ?1 AND symbol = ?2",
        params![exchange, symbol],
        |row| row.get(0),
    )?;
    
    Ok(count as usize)
}

/// Save config value
pub fn save_config(key: &str, value: &str) -> Result<(), rusqlite::Error> {
    let conn = DB.lock();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value, updated_at) VALUES (?1, ?2, ?3)",
        params![key, value, now],
    )?;
    Ok(())
}

/// Load config value
pub fn load_config(key: &str) -> Result<Option<String>, rusqlite::Error> {
    let conn = DB.lock();
    let result: Result<String, _> = conn.query_row(
        "SELECT value FROM config WHERE key = ?1",
        params![key],
        |row| row.get(0),
    );
    
    match result {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Save footprint bar
pub fn save_footprint_bar(
    exchange: &str,
    symbol: &str,
    bar_time: u64,
    bar_data: &serde_json::Value,
) -> Result<(), rusqlite::Error> {
    let conn = DB.lock();
    let json = serde_json::to_string(bar_data).unwrap_or_default();
    
    conn.execute(
        "INSERT OR REPLACE INTO footprint_bars (exchange, symbol, bar_time, bar_data)
         VALUES (?1, ?2, ?3, ?4)",
        params![exchange, symbol, bar_time as i64, json],
    )?;
    Ok(())
}

/// Get footprint bars for a ticker
pub fn get_footprint_bars(
    exchange: &str,
    symbol: &str,
    since: u64,
    limit: usize,
) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
    let conn = DB.lock();
    let mut stmt = conn.prepare(
        "SELECT bar_data FROM footprint_bars 
         WHERE exchange = ?1 AND symbol = ?2 AND bar_time > ?3
         ORDER BY bar_time ASC
         LIMIT ?4"
    )?;
    
    let bars = stmt.query_map(
        params![exchange, symbol, since as i64, limit as i64],
        |row| {
            let json: String = row.get(0)?;
            Ok(serde_json::from_str(&json).unwrap_or(serde_json::Value::Null))
        },
    )?.filter_map(|r| r.ok()).collect();
    
    Ok(bars)
}

/// Cleanup old data (7-day retention)
pub fn cleanup_old_data(retention_days: u64) -> Result<usize, rusqlite::Error> {
    let conn = DB.lock();
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64 - (retention_days as i64 * 24 * 60 * 60 * 1000);
    
    let trades_deleted = conn.execute(
        "DELETE FROM trades WHERE timestamp < ?1",
        params![cutoff],
    )?;
    
    let bars_deleted = conn.execute(
        "DELETE FROM footprint_bars WHERE bar_time < ?1",
        params![cutoff],
    )?;
    
    if trades_deleted > 0 || bars_deleted > 0 {
        log::info!("🗑️ Cleaned up {} old trades, {} old bars (>{} days)", 
            trades_deleted, bars_deleted, retention_days);
    }
    
    // Vacuum to reclaim space periodically
    conn.execute("PRAGMA optimize", [])?;
    
    Ok(trades_deleted + bars_deleted)
}

/// Get database statistics
pub fn get_stats() -> Result<serde_json::Value, rusqlite::Error> {
    let conn = DB.lock();
    
    let trade_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM trades",
        [],
        |row| row.get(0),
    )?;
    
    let bar_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM footprint_bars",
        [],
        |row| row.get(0),
    )?;
    
    let db_size = std::fs::metadata(DB_PATH)
        .map(|m| m.len())
        .unwrap_or(0);
    
    Ok(serde_json::json!({
        "total_trades": trade_count,
        "total_bars": bar_count,
        "database_size_bytes": db_size,
        "database_size_mb": db_size as f64 / 1024.0 / 1024.0,
    }))
}

// ============================================================================
// SERVER-SIDE FOOTPRINT AGGREGATION
// Runs continuously on the server, independent of browser connections
// ============================================================================

use std::collections::HashMap;
use parking_lot::RwLock;

/// Server-side footprint state for each ticker
pub struct ServerFootprintState {
    pub tick_buffer: Vec<TradeData>,
    pub bars: Vec<serde_json::Value>,
    pub tick_count: usize,
    pub tick_size: f64,
    pub last_price: f64,
    pub high_price: f64,
    pub low_price: f64,
    pub trade_count: u64,
}

#[derive(Clone)]
pub struct TradeData {
    pub timestamp: u64,
    pub price: f64,
    pub quantity: f64,
    pub is_buyer_maker: bool,
}

impl Default for ServerFootprintState {
    fn default() -> Self {
        Self {
            tick_buffer: Vec::new(),
            bars: Vec::new(),
            tick_count: 1000, // Default 1000T aggregation
            tick_size: 5.0,   // Default tick size
            last_price: 0.0,
            high_price: 0.0,
            low_price: f64::MAX,
            trade_count: 0,
        }
    }
}

/// Global server-side footprint states (one per active ticker)
pub static FOOTPRINT_STATES: Lazy<RwLock<HashMap<String, ServerFootprintState>>> = 
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Initialize footprint state for a ticker, loading existing bars from SQLite
fn init_footprint_state(exchange: &str, symbol: &str, initial_price: f64) -> ServerFootprintState {
    // Auto-detect tick size based on price
    let tick_size = if initial_price > 10000.0 { 5.0 }
        else if initial_price > 1000.0 { 1.0 }
        else if initial_price > 100.0 { 0.1 }
        else { 0.01 };
    
    let mut state = ServerFootprintState::default();
    state.tick_size = tick_size;
    
    // Load existing bars from SQLite (persisted from previous session)
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64 - (7 * 24 * 60 * 60 * 1000); // 7 days ago
    
    if let Ok(bars) = get_footprint_bars(exchange, symbol, cutoff, 200) {
        if !bars.is_empty() {
            log::info!("📊 Loaded {} existing footprint bars for {}:{} from SQLite", 
                bars.len(), exchange, symbol);
            state.bars = bars;
        }
    }
    
    state
}

/// Process a trade and aggregate into footprint bars (server-side)
/// This runs continuously regardless of browser connections
pub fn process_trade_for_footprint(
    exchange: &str,
    symbol: &str,
    timestamp: u64,
    price: f64,
    quantity: f64,
    is_buyer_maker: bool,
) {
    let key = format!("{}:{}", exchange, symbol);
    
    let mut states = FOOTPRINT_STATES.write();
    let state = states.entry(key.clone()).or_insert_with(|| {
        init_footprint_state(exchange, symbol, price)
    });
    
    // Update price tracking
    state.last_price = price;
    if price > state.high_price { state.high_price = price; }
    if price < state.low_price { state.low_price = price; }
    state.trade_count += 1;
    
    // Add to tick buffer
    state.tick_buffer.push(TradeData {
        timestamp,
        price,
        quantity,
        is_buyer_maker,
    });
    
    // Check if we have enough ticks to complete a bar
    if state.tick_buffer.len() >= state.tick_count {
        // Create footprint bar from buffer
        let bar = create_footprint_bar(&state.tick_buffer, state.tick_size);
        
        // Save to SQLite
        if let Err(e) = save_footprint_bar(exchange, symbol, bar["time"].as_u64().unwrap_or(timestamp), &bar) {
            log::error!("Failed to save footprint bar: {}", e);
        }
        
        // Keep in memory (limit to 200 bars)
        state.bars.push(bar);
        if state.bars.len() > 200 {
            state.bars.remove(0);
        }
        
        // Clear buffer
        state.tick_buffer.clear();
        
        // Log progress
        if state.trade_count % 10000 == 0 {
            log::info!("📊 {} server footprint: {} bars, {} trades total", 
                key, state.bars.len(), state.trade_count);
        }
    }
}

/// Create a footprint bar from trades
fn create_footprint_bar(trades: &[TradeData], tick_size: f64) -> serde_json::Value {
    if trades.is_empty() {
        return serde_json::json!({});
    }
    
    let open = trades[0].price;
    let close = trades[trades.len() - 1].price;
    let high = trades.iter().map(|t| t.price).fold(f64::MIN, f64::max);
    let low = trades.iter().map(|t| t.price).fold(f64::MAX, f64::min);
    let time = trades[0].timestamp;
    
    // Group by price level
    let mut price_levels: HashMap<i64, (f64, f64)> = HashMap::new(); // (bid, ask)
    
    for trade in trades {
        let level = (trade.price / tick_size).round() as i64;
        let entry = price_levels.entry(level).or_insert((0.0, 0.0));
        if trade.is_buyer_maker {
            entry.0 += trade.quantity; // bid (sell)
        } else {
            entry.1 += trade.quantity; // ask (buy)
        }
    }
    
    // Find POC (Point of Control)
    let poc_level = price_levels.iter()
        .max_by(|a, b| {
            let vol_a = a.1.0 + a.1.1;
            let vol_b = b.1.0 + b.1.1;
            vol_a.partial_cmp(&vol_b).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(level, _)| *level as f64 * tick_size)
        .unwrap_or(close);
    
    // Convert price levels to JSON
    let levels_json: HashMap<String, serde_json::Value> = price_levels.iter()
        .map(|(level, (bid, ask))| {
            let price = *level as f64 * tick_size;
            (format!("{:.2}", price), serde_json::json!({
                "bid": bid,
                "ask": ask,
                "delta": ask - bid
            }))
        })
        .collect();
    
    serde_json::json!({
        "time": time,
        "open": open,
        "high": high,
        "low": low,
        "close": close,
        "priceLevels": levels_json,
        "pocPrice": poc_level,
        "isBullish": close >= open,
        "tradeCount": trades.len()
    })
}

/// Get server-side footprint state for a ticker
pub fn get_footprint_state(exchange: &str, symbol: &str) -> Option<serde_json::Value> {
    let key = format!("{}:{}", exchange, symbol);
    let states = FOOTPRINT_STATES.read();
    
    states.get(&key).map(|state| {
        serde_json::json!({
            "ticker": key,
            "bars": state.bars,
            "current_buffer_size": state.tick_buffer.len(),
            "tick_count": state.tick_count,
            "tick_size": state.tick_size,
            "last_price": state.last_price,
            "high_price": state.high_price,
            "low_price": state.low_price,
            "total_trades": state.trade_count,
        })
    })
}

/// Get all server-side footprint states
pub fn get_all_footprint_states() -> serde_json::Value {
    let states = FOOTPRINT_STATES.read();
    
    let tickers: Vec<serde_json::Value> = states.iter()
        .map(|(key, state)| {
            serde_json::json!({
                "ticker": key,
                "bars_count": state.bars.len(),
                "buffer_size": state.tick_buffer.len(),
                "total_trades": state.trade_count,
                "last_price": state.last_price,
            })
        })
        .collect();
    
    serde_json::json!({
        "active_tickers": tickers,
        "count": tickers.len()
    })
}
