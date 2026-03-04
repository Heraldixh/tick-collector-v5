//! Application state management

use rustc_hash::FxHashMap;
use std::collections::VecDeque;
use serde::{Deserialize, Serialize};

/// Maximum trades to keep in memory per symbol (reduced for f1-micro 0.6GB RAM)
const MAX_TRADES_PER_SYMBOL: usize = 2_000;

/// Maximum candles to keep in memory per symbol/timeframe
const MAX_CANDLES_PER_SYMBOL: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub timestamp: u64,
    pub price: f64,
    pub quantity: f64,
    pub is_buyer_maker: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub timestamp: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub trade_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerInfo {
    pub exchange: String,
    pub symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub price: f64,
    pub change_24h: f64,
    pub volume_24h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartConfig {
    pub pane_tickers: [Option<String>; 9], // "binance:BTCUSDT" format - what browser displays
    pub filters: FilterConfig,
    #[serde(default)]
    pub active_tickers: Vec<String>, // Tickers for 24/7 collection - persists independently of browser
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FilterConfig {
    pub large_volume_only: bool,
    pub market_type: String, // "all", "spot", "perps"
    pub selected_exchanges: Vec<String>,
}

impl Default for ChartConfig {
    fn default() -> Self {
        Self {
            pane_tickers: [None, None, None, None, None, None, None, None, None],
            filters: FilterConfig::default(),
            active_tickers: Vec::new(),
        }
    }
}

/// Symbol key: "exchange:symbol" (e.g., "binance:BTCUSDT")
pub type SymbolKey = String;

pub struct AppState {
    /// Available tickers from exchanges
    pub tickers: FxHashMap<SymbolKey, TickerInfo>,
    
    /// Recent trades per symbol (ring buffer)
    pub trades: FxHashMap<SymbolKey, VecDeque<Trade>>,
    
    /// Aggregated candles per symbol (1-minute timeframe)
    pub candles: FxHashMap<SymbolKey, VecDeque<Candle>>,
    
    /// Current chart configuration
    pub config: ChartConfig,
    
    /// Active WebSocket subscribers per symbol
    pub subscribers: FxHashMap<SymbolKey, Vec<tokio::sync::mpsc::UnboundedSender<String>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            tickers: FxHashMap::default(),
            trades: FxHashMap::default(),
            candles: FxHashMap::default(),
            config: ChartConfig::default(),
            subscribers: FxHashMap::default(),
        }
    }
    
    pub fn add_trade(&mut self, key: &str, trade: Trade) {
        let trades = self.trades.entry(key.to_string()).or_insert_with(VecDeque::new);
        
        trades.push_back(trade.clone());
        
        // Limit memory usage
        while trades.len() > MAX_TRADES_PER_SYMBOL {
            trades.pop_front();
        }
        
        // Update ticker price
        if let Some(ticker) = self.tickers.get_mut(key) {
            ticker.price = trade.price;
        }
        
        // Broadcast to WebSocket subscribers and clean up dead ones
        if let Some(subs) = self.subscribers.get_mut(key) {
            let msg = serde_json::to_string(&trade).unwrap_or_default();
            let initial_count = subs.len();
            
            // Remove dead subscribers (those that fail to send)
            subs.retain(|tx| tx.send(msg.clone()).is_ok());
            
            let sent = subs.len();
            if sent > 0 {
                log::debug!("Broadcast trade to {} subscribers for {}", sent, key);
            }
            if initial_count != sent {
                log::info!("Cleaned up {} dead subscribers for {}", initial_count - sent, key);
            }
        }
    }
    
    pub fn add_candle(&mut self, key: &str, candle: Candle) {
        let candles = self.candles.entry(key.to_string()).or_insert_with(VecDeque::new);
        
        // Check if we should update the last candle or add new one
        if let Some(last) = candles.back_mut() {
            if last.timestamp == candle.timestamp {
                // Update existing candle
                last.high = last.high.max(candle.high);
                last.low = last.low.min(candle.low);
                last.close = candle.close;
                last.volume += candle.volume;
                last.trade_count += candle.trade_count;
                return;
            }
        }
        
        candles.push_back(candle);
        
        // Limit memory usage
        while candles.len() > MAX_CANDLES_PER_SYMBOL {
            candles.pop_front();
        }
    }
    
    pub fn get_trades(&self, key: &str, limit: usize) -> Vec<Trade> {
        self.trades
            .get(key)
            .map(|t| t.iter().rev().take(limit).cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    }
    
    pub fn get_candles(&self, key: &str, limit: usize) -> Vec<Candle> {
        self.candles
            .get(key)
            .map(|c| c.iter().rev().take(limit).cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    }
    
    pub fn subscribe(&mut self, key: &str) -> tokio::sync::mpsc::UnboundedReceiver<String> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let subs = self.subscribers
            .entry(key.to_string())
            .or_insert_with(Vec::new);
        
        // Limit subscribers per ticker to prevent memory issues
        if subs.len() >= 10 {
            log::warn!("Max subscribers (10) reached for {}, removing oldest", key);
            subs.remove(0);
        }
        
        subs.push(tx);
        log::info!("New subscriber for key '{}', total subscribers: {}", key, subs.len());
        rx
    }
}
