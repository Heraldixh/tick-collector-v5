# 🌐 Tick Collector Web - GCP Free Tier Deployment

## Overview

Deploy the Tick Collector web dashboard on GCP **completely free** using the always-free f1-micro tier.

| Resource | Specification | Monthly Cost |
|----------|--------------|--------------|
| VM | f1-micro (1 shared vCPU, 0.6GB RAM) | **$0** |
| Disk | 30GB Standard PD | **$0** |
| Static IP | Standard tier | **$0** |
| Network | 1GB egress/month | **$0** |
| **Total** | | **$0/month** |

---

## Quick Start

### Step 1: Prerequisites

```bash
# Install gcloud CLI
# https://cloud.google.com/sdk/docs/install

# Login and set project
gcloud auth login
gcloud config set project YOUR_PROJECT_ID
export GCP_PROJECT_ID="YOUR_PROJECT_ID"

# Enable Compute Engine API
gcloud services enable compute.googleapis.com
```

### Step 2: Provision VM

```bash
./1-provision-vm.sh
```

Wait 2-3 minutes for the VM to initialize.

### Step 3: Deploy Application

```bash
./2-deploy-app.sh
```

### Step 4: Setup Monitoring

```bash
./3-setup-service.sh
```

### Step 5: Access Dashboard

Open your browser to: `http://YOUR_STATIC_IP:8080`

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    GCP f1-micro VM ($0/month)               │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │           tick-collector-web (Rust/Actix)           │   │
│  │                                                     │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌────────────┐  │   │
│  │  │  REST API   │  │  WebSocket  │  │   Static   │  │   │
│  │  │  /api/v1/*  │  │  /ws/live/* │  │  Files /*  │  │   │
│  │  └─────────────┘  └─────────────┘  └────────────┘  │   │
│  │         │                │                         │   │
│  │         ▼                ▼                         │   │
│  │  ┌─────────────────────────────────────────────┐  │   │
│  │  │        Exchange WebSocket Connections        │  │   │
│  │  │  Binance | Bybit | OKX | Hyperliquid        │  │   │
│  │  └─────────────────────────────────────────────┘  │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  /opt/tick-collector/                                       │
│  ├── bin/tick-collector-web                                 │
│  └── static/{index.html, app.js, style.css}                │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼ HTTP/WebSocket
┌─────────────────────────────────────────────────────────────┐
│                        Browser                              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Tick Collector Dashboard                           │   │
│  │  ┌─────────┐ ┌───────────────────────────────────┐ │   │
│  │  │ Sidebar │ │         3x3 Chart Grid            │ │   │
│  │  │ Tickers │ │  [BTC] [ETH] [SOL]                │ │   │
│  │  │ Filters │ │  [XRP] [DOGE] [...]               │ │   │
│  │  └─────────┘ └───────────────────────────────────┘ │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

---

## API Endpoints

### REST API

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/health` | GET | Health check |
| `/api/v1/tickers` | GET | List available tickers |
| `/api/v1/tickers/{exchange}/{symbol}/candles` | GET | Get candle data |
| `/api/v1/tickers/{exchange}/{symbol}/trades` | GET | Get recent trades |
| `/api/v1/config` | GET | Get chart configuration |
| `/api/v1/config` | POST | Save chart configuration |

### WebSocket

| Endpoint | Description |
|----------|-------------|
| `/ws/live/{exchange}/{symbol}` | Real-time trade stream |

---

## Management Commands

```bash
# SSH into VM
gcloud compute ssh tick-collector-web --zone=us-west1-b

# View logs
gcloud compute ssh tick-collector-web --zone=us-west1-b \
  --command='journalctl -u tick-collector-web -f'

# Restart service
gcloud compute ssh tick-collector-web --zone=us-west1-b \
  --command='sudo systemctl restart tick-collector-web'

# Check status
gcloud compute ssh tick-collector-web --zone=us-west1-b \
  --command='systemctl status tick-collector-web'

# Stop VM (if needed)
gcloud compute instances stop tick-collector-web --zone=us-west1-b

# Start VM
gcloud compute instances start tick-collector-web --zone=us-west1-b
```

---

## Troubleshooting

### Service won't start

```bash
# Check logs
journalctl -u tick-collector-web -n 50

# Check if binary exists
ls -la /opt/tick-collector/bin/

# Test manually
/opt/tick-collector/bin/tick-collector-web
```

### Out of memory

The f1-micro has only 0.6GB RAM. The service is configured with memory limits:
- `MemoryMax=400M`
- `MemoryHigh=350M`

If you see OOM errors, the health check will auto-restart the service.

### Can't connect

1. Check firewall: `gcloud compute firewall-rules list`
2. Check VM is running: `gcloud compute instances list`
3. Check service: `systemctl status tick-collector-web`

---

## Cost Monitoring

```bash
# View billing
gcloud billing accounts list

# Set budget alert ($1 to catch unexpected charges)
# Go to: GCP Console → Billing → Budgets & alerts
```

---

## Updating

To deploy a new version:

```bash
# Rebuild and redeploy
./2-deploy-app.sh
```

---

**Happy Trading! 📈**
