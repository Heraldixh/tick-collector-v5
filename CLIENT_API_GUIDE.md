# Client API Guide — Tick Collector v5

> **Last Updated**: 2026-03-04
> **Server**: tick-collector-v5 running on GCP Free Tier VM
> **Features**: 24/7 data collection, SQLite persistence, server-side footprint aggregation, admin dashboard

---

## Server Architecture Overview

The server (`tick-collector-v5`) is a Rust/Actix-web application that:

1. **Connects to exchange WebSockets 24/7** — Binance, Bybit, OKX, Hyperliquid
2. **Receives every raw trade in real-time** via persistent WS connections with auto-reconnect
3. **Stores trades in SQLite database** for multi-client persistence (7-day retention)
4. **Aggregates trades into footprint bars server-side** — runs continuously regardless of browser state
5. **Serves real-time data via WebSocket** and historical data via REST API
6. **Provides admin dashboard** for monitoring and controlling data collection

```
                                    ┌─────────────────────────────────────────┐
                                    │           SERVER (24/7)                 │
                                    │                                         │
Exchange WS ──► start_single_stream │──► insert_trade() ──► SQLite DB        │
     │              │               │         │                               │
     │              │               │         ▼                               │
     │              │               │  process_trade_for_footprint()          │
     │              │               │         │                               │
     │              │               │         ▼                               │
     │              │               │  ServerFootprintState (in-memory)       │
     │              │               │  + save_footprint_bar() → SQLite        │
     │              ▼               │                                         │
     │         WebSocket ──────────►│──► Browser (real-time price updates)   │
     │                              │                                         │
     │                              │  /api/v1/server-footprint ◄── Browser  │
     │                              │  (polls every 2s for aggregated bars)   │
                                    └─────────────────────────────────────────┘
```

### Key Design Facts

- **Source of truth is SQLite.** All trades are stored in `data/tick_collector.db` with 7-day retention.
- **Server-side footprint aggregation.** The server continuously aggregates trades into footprint bars (1000T default), independent of browser connections.
- **Persistent streams via `active_tickers`.** Tickers added to the dashboard are stored in `active_tickers` and continue collecting even when the browser is closed.
- **Browser polls server for bars.** The admin dashboard polls `/api/v1/server-footprint/{exchange}/{symbol}` every 2 seconds to display the latest aggregated bars.
- **Auto-reconnect on all streams.** Each exchange stream has automatic reconnection with 3-second delay.
- **7-day data retention.** Old trades and footprint bars are automatically cleaned up.
- **Authentication required.** Admin dashboard requires login (configured on first run).

---

## API Endpoints

### 1. Authentication

```
GET  /api/v1/auth/status     # Check if authenticated
POST /api/v1/auth/setup      # First-time admin setup
POST /api/v1/auth/login      # Login with credentials
POST /api/v1/auth/logout     # Logout
```

On first run, visit the dashboard to set up admin credentials.

---

### 2. Get Available Tickers

```
GET /api/v1/tickers
```

Returns all available tickers from connected exchanges with current prices.

**Response:**
```json
[
  {
    "exchange": "binance",
    "symbol": "BTCUSDT",
    "price": 73150.5,
    "volume_24h": 1234567.89,
    "change_24h": 2.5
  }
]
```

---

### 3. Server-Side Footprint Data (24/7 Aggregated)

```
GET /api/v1/server-footprint
```

Returns all active server-side footprint states.

**Response:**
```json
{
  "active_tickers": [
    {
      "ticker": "binance:BTCUSDT",
      "bars_count": 45,
      "buffer_size": 234,
      "total_trades": 156789,
      "last_price": 73150.5
    }
  ],
  "count": 1
}
```

---

### 4. Get Footprint Data for Specific Ticker

```
GET /api/v1/server-footprint/{exchange}/{symbol}
```

Returns the server-side aggregated footprint bars for a specific ticker.

**Response:**
```json
{
  "ticker": "binance:BTCUSDT",
  "bars": [
    {
      "time": 1709571234567,
      "open": 73100.0,
      "high": 73200.0,
      "low": 73050.0,
      "close": 73150.0,
      "priceLevels": {
        "73100.00": { "bid": 12.5, "ask": 8.3, "delta": -4.2 },
        "73105.00": { "bid": 5.2, "ask": 15.1, "delta": 9.9 }
      },
      "pocPrice": 73105.0,
      "isBullish": true,
      "tradeCount": 1000
    }
  ],
  "current_buffer_size": 234,
  "tick_count": 1000,
  "tick_size": 5.0,
  "last_price": 73150.5,
  "high_price": 73500.0,
  "low_price": 72800.0,
  "total_trades": 156789
}
```

**Key fields:**
| Field | Description |
|-------|-------------|
| `bars` | Array of completed footprint bars (aggregated on server 24/7) |
| `current_buffer_size` | Trades in current forming bar |
| `tick_count` | Trades per bar (default: 1000) |
| `tick_size` | Price level granularity |
| `total_trades` | Total trades processed since stream started |

---

### 5. Active Tickers Management

```
GET    /api/v1/active-tickers                      # List all 24/7 collection tickers
DELETE /api/v1/active-tickers/{exchange}/{symbol}  # Stop collecting a ticker
```

**GET Response:**
```json
{
  "active_tickers": ["binance:BTCUSDT", "bybit:ETHUSDT"],
  "count": 2
}
```

Active tickers persist in `data/config.json` and continue collecting even when browser is closed.

---

### 6. Configuration Management

```
GET  /api/v1/config       # Get current config (pane_tickers, active_tickers)
POST /api/v1/config       # Save config (adds to active_tickers, never removes)
```

**POST Body:**
```json
{
  "pane_tickers": ["binance:BTCUSDT", null, null, null, null, null, null, null, null]
}
```

When saving config, any new tickers in `pane_tickers` are automatically added to `active_tickers` for 24/7 collection.

---

### 7. Real-Time WebSocket

```
WS /ws/live/{exchange}/{symbol}
```

Connect to receive real-time trade data for a specific ticker.

**Message format (received):**
```json
{
  "timestamp": 1709571234567,
  "price": 73150.5,
  "quantity": 0.125,
  "is_buyer_maker": false
}
```

---

### 8. Database Statistics

```
GET /api/v1/db/stats
```

Returns SQLite database statistics.

**Response:**
```json
{
  "trades_count": 1234567,
  "footprint_bars_count": 890,
  "db_size_bytes": 52428800,
  "oldest_trade": 1709484834567,
  "newest_trade": 1709571234567
}
```

---

### 9. Backup Trades (REST API fallback)

```
GET /api/v1/backup-trades/{exchange}/{symbol}?limit=1000
```

Fetch recent trades from exchange REST API. Used as backup when WebSocket has gaps.

---

### 10. Sync Endpoints (for data recovery)

```
GET /api/v1/sync/{exchange}/{symbol}/latest   # Get latest trade timestamp
GET /api/v1/sync/{exchange}/{symbol}/trades   # Get trades from SQLite
GET /api/v1/sync/{exchange}/{symbol}/bars     # Get footprint bars from SQLite
```

---

## Server Data Pipeline (Detailed)

### Step 1: Exchange WebSocket Ingestion

Each exchange has a dedicated stream managed by `start_single_stream()`:
- Connects to the exchange WS (Binance, Bybit, OKX, Hyperliquid)
- Subscribes to trade streams for the configured symbol
- Parses raw messages into a unified `Trade` struct
- Auto-reconnects with 3-second delay on failure
- Checks `active_tickers` config to determine if stream should continue

### Step 2: Trade Processing & Storage

For each incoming trade:
1. **Insert to SQLite** via `insert_trade()` — immediate persistence
2. **Process for footprint** via `process_trade_for_footprint()`:
   - Add to `tick_buffer` in `ServerFootprintState`
   - When buffer reaches `tick_count` (default 1000), create footprint bar
   - Save completed bar to SQLite via `save_footprint_bar()`
3. **Broadcast to WebSocket clients** for real-time display

### Step 3: Server-Side Footprint Aggregation

The `ServerFootprintState` for each ticker maintains:
- `tick_buffer`: Trades accumulating for current bar
- `bars`: Completed footprint bars (in-memory, also persisted to SQLite)
- `tick_count`: Trades per bar (default: 1000)
- `tick_size`: Price level granularity (auto-detected from price)

**This runs 24/7 regardless of browser connections.**

### Step 4: Browser Display

The admin dashboard:
1. Loads existing bars from `/api/v1/server-footprint/{exchange}/{symbol}` on connect
2. Polls every 2 seconds for new bars
3. Receives real-time price updates via WebSocket
4. Renders footprint chart with server-aggregated bars

---

## Data Flow Diagram

```
┌──────────────────────────────────────────────────────────────────────────┐
│                           SERVER (24/7)                                  │
│                                                                          │
│  Exchange WS ──► start_single_stream() ──► Trade                        │
│                         │                    │                           │
│                         │                    ├──► insert_trade() ──► SQLite (trades table)
│                         │                    │                           │
│                         │                    └──► process_trade_for_footprint()
│                         │                              │                 │
│                         │                              ▼                 │
│                         │                    ServerFootprintState        │
│                         │                    (tick_buffer, bars)         │
│                         │                              │                 │
│                         │                              ▼ (when tick_count reached)
│                         │                    save_footprint_bar() ──► SQLite (footprint_bars)
│                         │                                                │
│                         ▼                                                │
│                    WebSocket ──────────────────────────────────────────► Browser
│                                                                          │
│  /api/v1/server-footprint ◄────────────────────────────────────────────  │
│  (polls every 2s)                                                        │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## SQLite Database Schema

**Location:** `data/tick_collector.db`

### Tables

**trades**
```sql
CREATE TABLE trades (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    exchange TEXT NOT NULL,
    symbol TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    price REAL NOT NULL,
    quantity REAL NOT NULL,
    is_buyer_maker INTEGER NOT NULL,
    created_at INTEGER DEFAULT (strftime('%s', 'now'))
);
CREATE INDEX idx_trades_lookup ON trades(exchange, symbol, timestamp);
```

**footprint_bars**
```sql
CREATE TABLE footprint_bars (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    exchange TEXT NOT NULL,
    symbol TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    bar_data TEXT NOT NULL,  -- JSON blob
    created_at INTEGER DEFAULT (strftime('%s', 'now'))
);
CREATE INDEX idx_footprint_lookup ON footprint_bars(exchange, symbol, timestamp);
```

---

## Configuration Files

### data/config.json

Stores dashboard configuration and active tickers:

```json
{
  "pane_tickers": ["binance:BTCUSDT", null, null, null, null, null, null, null, null],
  "active_tickers": ["binance:BTCUSDT", "bybit:ETHUSDT"]
}
```

| Field | Description |
|-------|-------------|
| `pane_tickers` | Array of 9 pane assignments (browser display) |
| `active_tickers` | Tickers collecting 24/7 (persists across browser sessions) |

### data/auth.json

Stores admin credentials (created on first setup):

```json
{
  "username": "admin",
  "password_hash": "..."
}
```

---

## Available Exchanges

| Exchange | WebSocket URL | Symbol Format |
|----------|---------------|---------------|
| `binance` | `wss://fstream.binance.com` | `BTCUSDT` |
| `bybit` | `wss://stream.bybit.com` | `BTCUSDT` |
| `okx` | `wss://ws.okx.com` | `BTC-USDT-SWAP` |
| `hyperliquid` | `wss://api.hyperliquid.xyz` | `BTC` |

---

## Data Retention

- **Trades:** 7 days (configurable via `DATA_RETENTION_DAYS`)
- **Footprint bars:** 7 days
- **Cleanup:** Automatic background task runs every 30 seconds

---

## Deployment

### GCP Free Tier VM (f1-micro)

The server is optimized for GCP Free Tier:
- **Single worker** to minimize memory usage
- **SQLite** for lightweight persistence
- **Swap space** recommended (1GB) to prevent OOM kills

### Systemd Service

```ini
[Unit]
Description=Tick Collector Web Server
After=network.target

[Service]
Type=simple
User=tick-collector
WorkingDirectory=/opt/tick-collector
ExecStart=/opt/tick-collector/tick-collector-web
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### Directory Structure

```
/opt/tick-collector/
├── tick-collector-web     # Binary
├── static/                # HTML/JS/CSS
│   ├── index.html
│   ├── app.js
│   └── styles.css
├── data/                  # Persistent data
│   ├── tick_collector.db  # SQLite database
│   ├── config.json        # Dashboard config
│   └── auth.json          # Admin credentials
└── src/                   # Source code (for rebuilds)
```

---

## Admin Dashboard Usage

### First-Time Setup

1. Navigate to `http://SERVER_IP:8080`
2. Create admin username and password
3. Login to access dashboard

### Adding Tickers for 24/7 Collection

1. Click on a ticker in the sidebar
2. Assign to a pane
3. Ticker is automatically added to `active_tickers`
4. Data collection continues even when browser is closed

### Removing Tickers from Collection

Use the API:
```bash
curl -X DELETE http://SERVER_IP:8080/api/v1/active-tickers/binance/BTCUSDT
```

### Monitoring

- Check server logs: `sudo journalctl -u tick-collector -f`
- Check database stats: `GET /api/v1/db/stats`
- Check active tickers: `GET /api/v1/active-tickers`
- Check footprint states: `GET /api/v1/server-footprint`
