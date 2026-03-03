//! Exchange WebSocket connections for real-time market data
//! Supports: Binance, Bybit, OKX, Hyperliquid

use std::sync::Arc;
use parking_lot::RwLock;
use bytes::Bytes;
use fastwebsockets::{FragmentCollector, OpCode};
use http_body_util::Empty;
use hyper::{Request, header::{CONNECTION, UPGRADE}, upgrade::Upgraded};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, rustls::{ClientConfig, OwnedTrustAnchor}};
use serde::Deserialize;

use crate::state::{AppState, Trade, TickerInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exchange {
    Binance,
    Bybit,
    Okx,
    Hyperliquid,
}

impl Exchange {
    pub fn as_str(&self) -> &'static str {
        match self {
            Exchange::Binance => "binance",
            Exchange::Bybit => "bybit",
            Exchange::Okx => "okx",
            Exchange::Hyperliquid => "hyperliquid",
        }
    }
    
    pub fn ws_domain(&self) -> &'static str {
        match self {
            Exchange::Binance => "fstream.binance.com",
            Exchange::Bybit => "stream.bybit.com",
            Exchange::Okx => "ws.okx.com",
            Exchange::Hyperliquid => "api.hyperliquid.xyz",
        }
    }
    
    pub fn rest_url(&self) -> &'static str {
        match self {
            Exchange::Binance => "https://fapi.binance.com/fapi/v1/exchangeInfo",
            Exchange::Bybit => "https://api.bybit.com/v5/market/instruments-info?category=linear",
            Exchange::Okx => "https://www.okx.com/api/v5/public/instruments?instType=SWAP",
            Exchange::Hyperliquid => "https://api.hyperliquid.xyz/info",
        }
    }
}

/// Fetch all available tickers from all exchanges
pub async fn fetch_all_tickers() -> Vec<(String, String, TickerInfo)> {
    let mut all_tickers = Vec::new();
    
    // Fetch from all exchanges in parallel
    let (binance, bybit, okx, hyperliquid) = tokio::join!(
        fetch_binance_tickers(),
        fetch_bybit_tickers(),
        fetch_okx_tickers(),
        fetch_hyperliquid_tickers(),
    );
    
    all_tickers.extend(binance.unwrap_or_default());
    all_tickers.extend(bybit.unwrap_or_default());
    all_tickers.extend(okx.unwrap_or_default());
    all_tickers.extend(hyperliquid.unwrap_or_default());
    
    log::info!("Fetched {} total tickers from all exchanges", all_tickers.len());
    all_tickers
}

async fn fetch_binance_tickers() -> Result<Vec<(String, String, TickerInfo)>, Box<dyn std::error::Error + Send + Sync>> {
    // Fetch 24hr ticker stats which includes volume
    #[derive(Deserialize)]
    struct BinanceTicker24hr {
        symbol: String,
        #[serde(rename = "priceChangePercent")]
        price_change_percent: String,
        #[serde(rename = "lastPrice")]
        last_price: String,
        #[serde(rename = "quoteVolume")]
        quote_volume: String,
    }
    
    let resp: Vec<BinanceTicker24hr> = reqwest::get("https://fapi.binance.com/fapi/v1/ticker/24hr")
        .await?
        .json()
        .await?;
    
    let tickers: Vec<_> = resp
        .into_iter()
        .filter(|s| s.symbol.ends_with("USDT"))
        .map(|s| {
            let base_asset = s.symbol.trim_end_matches("USDT").to_string();
            let key = format!("binance:{}", s.symbol);
            let info = TickerInfo {
                exchange: "binance".to_string(),
                symbol: s.symbol.clone(),
                base_asset,
                quote_asset: "USDT".to_string(),
                price: s.last_price.parse().unwrap_or(0.0),
                change_24h: s.price_change_percent.parse().unwrap_or(0.0),
                volume_24h: s.quote_volume.parse().unwrap_or(0.0),
            };
            (key, s.symbol, info)
        })
        .collect();
    
    log::info!("Fetched {} Binance tickers", tickers.len());
    Ok(tickers)
}

async fn fetch_bybit_tickers() -> Result<Vec<(String, String, TickerInfo)>, Box<dyn std::error::Error + Send + Sync>> {
    // Fetch tickers with volume data
    #[derive(Deserialize)]
    struct BybitResponse {
        result: BybitResult,
    }
    #[derive(Deserialize)]
    struct BybitResult {
        list: Vec<BybitTicker>,
    }
    #[derive(Deserialize)]
    struct BybitTicker {
        symbol: String,
        #[serde(rename = "lastPrice")]
        last_price: String,
        #[serde(rename = "price24hPcnt")]
        price_24h_pcnt: String,
        #[serde(rename = "turnover24h")]
        turnover_24h: String,
    }
    
    let resp: BybitResponse = reqwest::get("https://api.bybit.com/v5/market/tickers?category=linear")
        .await?
        .json()
        .await?;
    
    let tickers: Vec<_> = resp.result.list
        .into_iter()
        .filter(|s| s.symbol.ends_with("USDT"))
        .map(|s| {
            let base_asset = s.symbol.trim_end_matches("USDT").to_string();
            let key = format!("bybit:{}", s.symbol);
            let info = TickerInfo {
                exchange: "bybit".to_string(),
                symbol: s.symbol.clone(),
                base_asset,
                quote_asset: "USDT".to_string(),
                price: s.last_price.parse().unwrap_or(0.0),
                change_24h: s.price_24h_pcnt.parse::<f64>().unwrap_or(0.0) * 100.0,
                volume_24h: s.turnover_24h.parse().unwrap_or(0.0),
            };
            (key, s.symbol, info)
        })
        .collect();
    
    log::info!("Fetched {} Bybit tickers", tickers.len());
    Ok(tickers)
}

async fn fetch_okx_tickers() -> Result<Vec<(String, String, TickerInfo)>, Box<dyn std::error::Error + Send + Sync>> {
    // Fetch tickers with volume data
    #[derive(Deserialize)]
    struct OkxResponse {
        data: Vec<OkxTicker>,
    }
    #[derive(Deserialize)]
    struct OkxTicker {
        #[serde(rename = "instId")]
        inst_id: String,
        last: String,
        #[serde(rename = "sodUtc0")]
        sod_utc0: String,
        #[serde(rename = "volCcy24h")]
        vol_ccy_24h: String,
    }
    
    let resp: OkxResponse = reqwest::get("https://www.okx.com/api/v5/market/tickers?instType=SWAP")
        .await?
        .json()
        .await?;
    
    let tickers: Vec<_> = resp.data
        .into_iter()
        .filter(|s| s.inst_id.contains("USDT"))
        .map(|s| {
            // OKX format: BTC-USDT-SWAP -> BTCUSDT
            let symbol = s.inst_id.replace("-SWAP", "").replace("-", "");
            let base_asset = symbol.trim_end_matches("USDT").to_string();
            let key = format!("okx:{}", symbol);
            let last_price: f64 = s.last.parse().unwrap_or(0.0);
            let sod_price: f64 = s.sod_utc0.parse().unwrap_or(0.0);
            let change_24h = if sod_price > 0.0 { ((last_price - sod_price) / sod_price) * 100.0 } else { 0.0 };
            let info = TickerInfo {
                exchange: "okx".to_string(),
                symbol: symbol.clone(),
                base_asset,
                quote_asset: "USDT".to_string(),
                price: last_price,
                change_24h,
                volume_24h: s.vol_ccy_24h.parse().unwrap_or(0.0),
            };
            (key, symbol, info)
        })
        .collect();
    
    log::info!("Fetched {} OKX tickers", tickers.len());
    Ok(tickers)
}

async fn fetch_hyperliquid_tickers() -> Result<Vec<(String, String, TickerInfo)>, Box<dyn std::error::Error + Send + Sync>> {
    // Fetch both meta and market data for volume
    #[derive(Deserialize)]
    struct HyperliquidMeta {
        universe: Vec<HyperliquidAsset>,
    }
    #[derive(Deserialize)]
    struct HyperliquidAsset {
        name: String,
    }
    
    #[derive(Deserialize)]
    struct HyperliquidMarket {
        coin: String,
        #[serde(rename = "dayNtlVlm")]
        day_ntl_vlm: String,
        #[serde(rename = "markPx")]
        mark_px: String,
        #[serde(rename = "prevDayPx")]
        prev_day_px: String,
    }
    
    let client = reqwest::Client::new();
    
    // Fetch meta for asset list
    let meta_resp: HyperliquidMeta = client
        .post(Exchange::Hyperliquid.rest_url())
        .json(&serde_json::json!({"type": "meta"}))
        .send()
        .await?
        .json()
        .await?;
    
    // Fetch market data for volume
    let market_resp: Vec<HyperliquidMarket> = client
        .post(Exchange::Hyperliquid.rest_url())
        .json(&serde_json::json!({"type": "allMids"}))
        .send()
        .await?
        .json()
        .await
        .unwrap_or_default();
    
    // Create a map of coin -> market data
    let market_map: std::collections::HashMap<String, &HyperliquidMarket> = market_resp
        .iter()
        .map(|m| (m.coin.clone(), m))
        .collect();
    
    let tickers: Vec<_> = meta_resp.universe
        .into_iter()
        .map(|s| {
            let symbol = format!("{}USDT", s.name);
            let key = format!("hyperliquid:{}", symbol);
            
            let (price, change_24h, volume_24h) = market_map.get(&s.name)
                .map(|m| {
                    let mark_px: f64 = m.mark_px.parse().unwrap_or(0.0);
                    let prev_px: f64 = m.prev_day_px.parse().unwrap_or(0.0);
                    let change = if prev_px > 0.0 { ((mark_px - prev_px) / prev_px) * 100.0 } else { 0.0 };
                    let vol: f64 = m.day_ntl_vlm.parse().unwrap_or(0.0);
                    (mark_px, change, vol)
                })
                .unwrap_or((0.0, 0.0, 0.0));
            
            let info = TickerInfo {
                exchange: "hyperliquid".to_string(),
                symbol: symbol.clone(),
                base_asset: s.name,
                quote_asset: "USDT".to_string(),
                price,
                change_24h,
                volume_24h,
            };
            (key, symbol, info)
        })
        .collect();
    
    log::info!("Fetched {} Hyperliquid tickers", tickers.len());
    Ok(tickers)
}

use std::collections::HashSet;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

// Track which streams are already running to avoid duplicates
static ACTIVE_STREAMS: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Start a persistent stream for a specific ticker (runs 24/7 for configured tickers)
/// This stream will auto-reconnect on disconnect to ensure continuous data collection
/// Data is collected regardless of whether any browser clients are connected
pub async fn start_single_stream(
    state: Arc<RwLock<AppState>>,
    exchange: &str,
    symbol: &str,
    key: &str,
) {
    // Check if stream is already running
    {
        let mut active = ACTIVE_STREAMS.lock();
        if active.contains(key) {
            log::debug!("Stream already active: {}", key);
            return;
        }
        active.insert(key.to_string());
    }
    
    log::info!("🚀 Starting PERSISTENT stream (24/7 collection): {}", key);
    
    let exchange_owned = exchange.to_string();
    let symbol_owned = symbol.to_string();
    let key_owned = key.to_string();
    
    // PERSISTENT auto-reconnect loop - runs 24/7 regardless of browser connections
    // Only stops if the ticker is explicitly removed from config
    loop {
        let result = match exchange_owned.as_str() {
            "binance" => connect_binance_stream(&key_owned, &symbol_owned, &state).await,
            "bybit" => connect_bybit_stream(&key_owned, &symbol_owned, &state).await,
            "okx" => connect_okx_stream(&key_owned, &symbol_owned, &state).await,
            "hyperliquid" => connect_hyperliquid_stream(&key_owned, &symbol_owned, &state).await,
            _ => {
                log::error!("Unknown exchange: {}", exchange_owned);
                break;
            }
        };
        
        match result {
            Ok(_) => {
                log::warn!("Stream {} ended normally, reconnecting immediately...", key_owned);
            }
            Err(e) => {
                log::error!("Stream {} error: {}, reconnecting in 3s...", key_owned, e);
            }
        }
        
        // Check if ticker is still in the saved config (admin hasn't removed it)
        let still_configured = check_ticker_in_config(&key_owned);
        
        if !still_configured {
            log::info!("Ticker {} removed from config, stopping stream", key_owned);
            break;
        }
        
        // Wait before reconnecting (shorter delay for faster recovery)
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        log::info!("🔄 Reconnecting stream: {}", key_owned);
    }
    
    // Remove from active streams when done
    {
        let mut active = ACTIVE_STREAMS.lock();
        active.remove(&key_owned);
    }
    
    log::info!("Stream {} stopped (removed from config)", key);
}

/// Check if a ticker is still in the saved config
fn check_ticker_in_config(key: &str) -> bool {
    let config_path = "data/config.json";
    if let Ok(contents) = std::fs::read_to_string(config_path) {
        if let Ok(config) = serde_json::from_str::<crate::state::ChartConfig>(&contents) {
            return config.pane_tickers.iter().any(|t| t.as_ref() == Some(&key.to_string()));
        }
    }
    // If config doesn't exist or can't be read, keep the stream running
    true
}

/// Start WebSocket connections to exchanges
/// This fetches ticker info for the sidebar and starts PERSISTENT 24/7 streams
/// for all tickers configured in the admin's chart panes.
/// Data collection runs continuously regardless of browser connections.
pub async fn start_exchange_connections(state: Arc<RwLock<AppState>>) {
    // Fetch all tickers from all exchanges (for sidebar list only)
    log::info!("Fetching tickers from all exchanges (for sidebar list)...");
    let all_tickers = fetch_all_tickers().await;
    
    // Initialize ticker info in state (metadata only, no data collection)
    {
        let mut state = state.write();
        for (key, _symbol, info) in &all_tickers {
            state.tickers.insert(key.clone(), info.clone());
        }
    }
    
    log::info!("📋 Loaded {} tickers for sidebar.", all_tickers.len());
    
    // Load saved config and start PERSISTENT 24/7 streams for configured tickers
    let config_path = "data/config.json";
    if let Ok(contents) = std::fs::read_to_string(config_path) {
        if let Ok(config) = serde_json::from_str::<crate::state::ChartConfig>(&contents) {
            let mut active_count = 0;
            for ticker_opt in &config.pane_tickers {
                if let Some(ticker_key) = ticker_opt {
                    if let Some((exchange, symbol)) = ticker_key.split_once(':') {
                        let state_clone = Arc::clone(&state);
                        let key = ticker_key.clone();
                        let exchange_owned = exchange.to_string();
                        let symbol_owned = symbol.to_string();
                        
                        // Start PERSISTENT stream - runs 24/7 regardless of browser connections
                        tokio::spawn(async move {
                            start_single_stream(state_clone, &exchange_owned, &symbol_owned, &key).await;
                        });
                        active_count += 1;
                    }
                }
            }
            if active_count > 0 {
                log::info!("🚀 Started {} PERSISTENT 24/7 streams from saved config", active_count);
            }
        }
    }
    
    log::info!("✅ Server ready. Data collection runs 24/7 for configured tickers.");
}

async fn connect_binance_stream(
    key: &str,
    symbol: &str,
    state: &Arc<RwLock<AppState>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let domain = "fstream.binance.com";
    let url = format!("/ws/{}@aggTrade", symbol.to_lowercase());
    
    let mut ws = connect_ws(domain, &url).await?;
    
    log::info!("Connected to binance:{} (key={})", symbol, key);
    
    let mut trade_count = 0u64;
    
    loop {
        let frame = ws.read_frame().await?;
        
        match frame.opcode {
            OpCode::Text | OpCode::Binary => {
                let data = frame.payload.to_vec();
                
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&data) {
                    if let (Some(price), Some(qty), Some(time), Some(is_buyer)) = (
                        json["p"].as_str(),
                        json["q"].as_str(),
                        json["T"].as_u64(),
                        json["m"].as_bool(),
                    ) {
                        let trade = Trade {
                            timestamp: time,
                            price: price.parse().unwrap_or(0.0),
                            quantity: qty.parse().unwrap_or(0.0),
                            is_buyer_maker: is_buyer,
                        };
                        
                        trade_count += 1;
                        if trade_count % 100 == 1 {
                            log::info!("Trade #{} for {}: price={}", trade_count, key, trade.price);
                        }
                        
                        let mut state = state.write();
                        state.add_trade(key, trade);
                    }
                }
            }
            OpCode::Close => {
                log::info!("WebSocket closed by server");
                return Ok(());
            }
            OpCode::Ping => {
                ws.write_frame(fastwebsockets::Frame::pong(frame.payload)).await?;
            }
            _ => {}
        }
    }
}

async fn connect_bybit_stream(
    key: &str,
    symbol: &str,
    state: &Arc<RwLock<AppState>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let domain = "stream.bybit.com";
    let url = "/v5/public/linear";
    
    let mut ws = connect_ws(domain, url).await?;
    
    // Subscribe to trades
    let sub_msg = serde_json::json!({
        "op": "subscribe",
        "args": [format!("publicTrade.{}", symbol)]
    });
    ws.write_frame(fastwebsockets::Frame::text(
        fastwebsockets::Payload::Borrowed(sub_msg.to_string().as_bytes())
    )).await?;
    
    log::info!("Connected to bybit:{}", symbol);
    
    loop {
        let frame = ws.read_frame().await?;
        
        match frame.opcode {
            OpCode::Text | OpCode::Binary => {
                let data = frame.payload.to_vec();
                
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&data) {
                    if let Some(trades) = json["data"].as_array() {
                        for t in trades {
                            if let (Some(price), Some(qty), Some(time), Some(side)) = (
                                t["p"].as_str(),
                                t["v"].as_str(),
                                t["T"].as_u64(),
                                t["S"].as_str(),
                            ) {
                                let trade = Trade {
                                    timestamp: time,
                                    price: price.parse().unwrap_or(0.0),
                                    quantity: qty.parse().unwrap_or(0.0),
                                    is_buyer_maker: side == "Sell",
                                };
                                
                                let mut state = state.write();
                                state.add_trade(key, trade);
                            }
                        }
                    }
                }
            }
            OpCode::Close => {
                return Ok(());
            }
            OpCode::Ping => {
                ws.write_frame(fastwebsockets::Frame::pong(frame.payload)).await?;
            }
            _ => {}
        }
    }
}

async fn connect_okx_stream(
    key: &str,
    symbol: &str,
    state: &Arc<RwLock<AppState>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let domain = "ws.okx.com";
    let url = "/ws/v5/public";
    
    let mut ws = connect_ws(domain, url).await?;
    
    // OKX format: BTCUSDT -> BTC-USDT-SWAP
    let inst_id = format!("{}-{}-SWAP", 
        &symbol[..symbol.len()-4], 
        &symbol[symbol.len()-4..]
    );
    
    let sub_msg = serde_json::json!({
        "op": "subscribe",
        "args": [{"channel": "trades", "instId": inst_id}]
    });
    ws.write_frame(fastwebsockets::Frame::text(
        fastwebsockets::Payload::Borrowed(sub_msg.to_string().as_bytes())
    )).await?;
    
    log::info!("Connected to okx:{}", symbol);
    
    loop {
        let frame = ws.read_frame().await?;
        
        match frame.opcode {
            OpCode::Text | OpCode::Binary => {
                let data = frame.payload.to_vec();
                
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&data) {
                    if let Some(trades) = json["data"].as_array() {
                        for t in trades {
                            if let (Some(price), Some(qty), Some(time), Some(side)) = (
                                t["px"].as_str(),
                                t["sz"].as_str(),
                                t["ts"].as_str(),
                                t["side"].as_str(),
                            ) {
                                let trade = Trade {
                                    timestamp: time.parse().unwrap_or(0),
                                    price: price.parse().unwrap_or(0.0),
                                    quantity: qty.parse().unwrap_or(0.0),
                                    is_buyer_maker: side == "sell",
                                };
                                
                                let mut state = state.write();
                                state.add_trade(key, trade);
                            }
                        }
                    }
                }
            }
            OpCode::Close => {
                return Ok(());
            }
            OpCode::Ping => {
                ws.write_frame(fastwebsockets::Frame::pong(frame.payload)).await?;
            }
            _ => {}
        }
    }
}

async fn connect_hyperliquid_stream(
    key: &str,
    symbol: &str,
    state: &Arc<RwLock<AppState>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let domain = "api.hyperliquid.xyz";
    let url = "/ws";
    
    let mut ws = connect_ws(domain, url).await?;
    
    // Extract base asset (remove USDT suffix)
    let coin = symbol.replace("USDT", "");
    
    let sub_msg = serde_json::json!({
        "method": "subscribe",
        "subscription": {"type": "trades", "coin": coin}
    });
    ws.write_frame(fastwebsockets::Frame::text(
        fastwebsockets::Payload::Borrowed(sub_msg.to_string().as_bytes())
    )).await?;
    
    log::info!("Connected to hyperliquid:{}", symbol);
    
    loop {
        let frame = ws.read_frame().await?;
        
        match frame.opcode {
            OpCode::Text | OpCode::Binary => {
                let data = frame.payload.to_vec();
                
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&data) {
                    if let Some(trades) = json["data"].as_array() {
                        for t in trades {
                            if let (Some(price), Some(qty), Some(time), Some(side)) = (
                                t["px"].as_str(),
                                t["sz"].as_str(),
                                t["time"].as_u64(),
                                t["side"].as_str(),
                            ) {
                                let trade = Trade {
                                    timestamp: time,
                                    price: price.parse().unwrap_or(0.0),
                                    quantity: qty.parse().unwrap_or(0.0),
                                    is_buyer_maker: side == "A",
                                };
                                
                                let mut state = state.write();
                                state.add_trade(key, trade);
                            }
                        }
                    }
                }
            }
            OpCode::Close => {
                return Ok(());
            }
            OpCode::Ping => {
                ws.write_frame(fastwebsockets::Frame::pong(frame.payload)).await?;
            }
            _ => {}
        }
    }
}

// ============================================================================
// REST API BACKUP - Fetch historical trades when WebSocket is disconnected
// ============================================================================

/// Fetch recent trades from exchange REST API (backup for WebSocket gaps)
pub async fn fetch_historical_trades(
    exchange: &str,
    symbol: &str,
    limit: usize,
) -> Result<Vec<Trade>, Box<dyn std::error::Error + Send + Sync>> {
    match exchange {
        "binance" => fetch_binance_trades(symbol, limit).await,
        "bybit" => fetch_bybit_trades(symbol, limit).await,
        "okx" => fetch_okx_trades(symbol, limit).await,
        "hyperliquid" => fetch_hyperliquid_trades(symbol, limit).await,
        _ => Err(format!("Unknown exchange: {}", exchange).into()),
    }
}

async fn fetch_binance_trades(
    symbol: &str,
    limit: usize,
) -> Result<Vec<Trade>, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(Deserialize)]
    struct BinanceTrade {
        #[serde(rename = "T")]
        time: u64,
        #[serde(rename = "p")]
        price: String,
        #[serde(rename = "q")]
        qty: String,
        #[serde(rename = "m")]
        is_buyer_maker: bool,
    }
    
    let url = format!(
        "https://fapi.binance.com/fapi/v1/aggTrades?symbol={}&limit={}",
        symbol, limit.min(1000)
    );
    
    let resp: Vec<BinanceTrade> = reqwest::get(&url).await?.json().await?;
    
    let trades: Vec<Trade> = resp.into_iter().map(|t| Trade {
        timestamp: t.time,
        price: t.price.parse().unwrap_or(0.0),
        quantity: t.qty.parse().unwrap_or(0.0),
        is_buyer_maker: t.is_buyer_maker,
    }).collect();
    
    log::info!("Fetched {} trades from Binance REST API for {}", trades.len(), symbol);
    Ok(trades)
}

async fn fetch_bybit_trades(
    symbol: &str,
    limit: usize,
) -> Result<Vec<Trade>, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(Deserialize)]
    struct BybitResponse {
        result: BybitResult,
    }
    #[derive(Deserialize)]
    struct BybitResult {
        list: Vec<BybitTrade>,
    }
    #[derive(Deserialize)]
    struct BybitTrade {
        #[serde(rename = "T")]
        time: u64,
        #[serde(rename = "p")]
        price: String,
        #[serde(rename = "v")]
        size: String,
        #[serde(rename = "S")]
        side: String,
    }
    
    let url = format!(
        "https://api.bybit.com/v5/market/recent-trade?category=linear&symbol={}&limit={}",
        symbol, limit.min(1000)
    );
    
    let resp: BybitResponse = reqwest::get(&url).await?.json().await?;
    
    let trades: Vec<Trade> = resp.result.list.into_iter().map(|t| Trade {
        timestamp: t.time,
        price: t.price.parse().unwrap_or(0.0),
        quantity: t.size.parse().unwrap_or(0.0),
        is_buyer_maker: t.side == "Sell",
    }).collect();
    
    log::info!("Fetched {} trades from Bybit REST API for {}", trades.len(), symbol);
    Ok(trades)
}

async fn fetch_okx_trades(
    symbol: &str,
    limit: usize,
) -> Result<Vec<Trade>, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(Deserialize)]
    struct OkxResponse {
        data: Vec<OkxTrade>,
    }
    #[derive(Deserialize)]
    struct OkxTrade {
        ts: String,
        px: String,
        sz: String,
        side: String,
    }
    
    // OKX format: BTCUSDT -> BTC-USDT-SWAP
    let inst_id = format!("{}-{}-SWAP", 
        &symbol[..symbol.len()-4], 
        &symbol[symbol.len()-4..]
    );
    
    let url = format!(
        "https://www.okx.com/api/v5/market/trades?instId={}&limit={}",
        inst_id, limit.min(500)
    );
    
    let resp: OkxResponse = reqwest::get(&url).await?.json().await?;
    
    let trades: Vec<Trade> = resp.data.into_iter().map(|t| Trade {
        timestamp: t.ts.parse().unwrap_or(0),
        price: t.px.parse().unwrap_or(0.0),
        quantity: t.sz.parse().unwrap_or(0.0),
        is_buyer_maker: t.side == "sell",
    }).collect();
    
    log::info!("Fetched {} trades from OKX REST API for {}", trades.len(), symbol);
    Ok(trades)
}

async fn fetch_hyperliquid_trades(
    symbol: &str,
    limit: usize,
) -> Result<Vec<Trade>, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(Deserialize)]
    struct HyperliquidTrade {
        time: u64,
        px: String,
        sz: String,
        side: String,
    }
    
    let coin = symbol.replace("USDT", "");
    
    let client = reqwest::Client::new();
    let resp: Vec<HyperliquidTrade> = client
        .post("https://api.hyperliquid.xyz/info")
        .json(&serde_json::json!({
            "type": "recentTrades",
            "coin": coin,
            "limit": limit.min(1000)
        }))
        .send()
        .await?
        .json()
        .await
        .unwrap_or_default();
    
    let trades: Vec<Trade> = resp.into_iter().map(|t| Trade {
        timestamp: t.time,
        price: t.px.parse().unwrap_or(0.0),
        quantity: t.sz.parse().unwrap_or(0.0),
        is_buyer_maker: t.side == "A",
    }).collect();
    
    log::info!("Fetched {} trades from Hyperliquid REST API for {}", trades.len(), symbol);
    Ok(trades)
}

// WebSocket connection helpers
async fn connect_ws(domain: &str, url: &str) -> Result<FragmentCollector<TokioIo<Upgraded>>, Box<dyn std::error::Error + Send + Sync>> {
    let tcp_stream = setup_tcp(domain).await?;
    let tls_stream = upgrade_to_tls(domain, tcp_stream).await?;
    Ok(upgrade_to_websocket(domain, tls_stream, url).await?)
}

struct SpawnExecutor;

impl<Fut> hyper::rt::Executor<Fut> for SpawnExecutor
where
    Fut: std::future::Future + Send + 'static,
    Fut::Output: Send + 'static,
{
    fn execute(&self, fut: Fut) {
        tokio::task::spawn(fut);
    }
}

async fn setup_tcp(domain: &str) -> Result<TcpStream, Box<dyn std::error::Error + Send + Sync>> {
    let addr = format!("{domain}:443");
    Ok(TcpStream::connect(&addr).await?)
}

fn tls_connector() -> Result<TlsConnector, Box<dyn std::error::Error + Send + Sync>> {
    let mut root_store = tokio_rustls::rustls::RootCertStore::empty();

    root_store.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
        OwnedTrustAnchor::from_subject_spki_name_constraints(
            ta.subject,
            ta.spki,
            ta.name_constraints,
        )
    }));

    let config = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(TlsConnector::from(std::sync::Arc::new(config)))
}

async fn upgrade_to_tls(
    domain: &str,
    tcp_stream: TcpStream,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, Box<dyn std::error::Error + Send + Sync>> {
    let domain_name = tokio_rustls::rustls::ServerName::try_from(domain)?;
    Ok(tls_connector()?.connect(domain_name, tcp_stream).await?)
}

async fn upgrade_to_websocket(
    domain: &str,
    tls_stream: tokio_rustls::client::TlsStream<TcpStream>,
    url: &str,
) -> Result<FragmentCollector<TokioIo<Upgraded>>, Box<dyn std::error::Error + Send + Sync>> {
    let req: Request<Empty<Bytes>> = Request::builder()
        .method("GET")
        .uri(url)
        .header("Host", domain)
        .header(UPGRADE, "websocket")
        .header(CONNECTION, "upgrade")
        .header("Sec-WebSocket-Key", fastwebsockets::handshake::generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(Empty::<Bytes>::new())?;

    let (ws, _) = fastwebsockets::handshake::client(&SpawnExecutor, req, tls_stream).await?;
    Ok(FragmentCollector::new(ws))
}
