# Tick Collector Web Server - GCP Free Tier Deployment Guide

## System Requirements

- **Instance**: GCP f1-micro (1 vCPU, 0.6GB RAM)
- **OS**: Debian 11+ or Ubuntu 20.04+
- **Storage**: 10GB+ (for 7-day data retention)
- **Network**: Port 8080 open

## Quick Start

### 1. Build the Binary (on your development machine)

```bash
# Cross-compile for Linux (from Windows)
cargo build --release --target x86_64-unknown-linux-gnu

# Or build directly on Linux
cargo build --release
```

### 2. Upload to Server

```bash
# Upload binary and static files
scp target/release/tick-collector-web user@YOUR_SERVER:/tmp/
scp -r static user@YOUR_SERVER:/tmp/
scp deploy/* user@YOUR_SERVER:/tmp/
```

### 3. Install on Server

```bash
ssh user@YOUR_SERVER
cd /tmp
sudo chmod +x install.sh
sudo ./install.sh
```

### 4. Start the Service

```bash
sudo systemctl start tick-collector
sudo systemctl status tick-collector
```

## Configuration

### First-Time Setup

1. Open `http://YOUR_SERVER:8080` in browser
2. Create admin account (first-time setup)
3. Login and assign tickers to chart panes
4. Data collection starts automatically

### API Key for Desktop Client

After admin setup, get your API key from:
- Browser: Admin tab → API Key section
- File: `/opt/tick-collector/data/api_key.txt`

## Management Commands

```bash
# Service control
sudo systemctl start tick-collector
sudo systemctl stop tick-collector
sudo systemctl restart tick-collector
sudo systemctl status tick-collector

# View logs
sudo journalctl -u tick-collector -f          # Follow logs
sudo journalctl -u tick-collector --since today  # Today's logs
sudo journalctl -u tick-collector -n 100      # Last 100 lines

# Check disk usage
du -sh /opt/tick-collector/data/

# Manual cleanup (if needed)
rm -rf /opt/tick-collector/data/trades/*
```

## API Endpoints

### Public (No Auth)
- `GET /api/v1/health` - Health check
- `GET /api/v1/auth/status` - Auth status

### Admin (Session Auth)
- `POST /api/v1/auth/setup` - First-time admin setup
- `POST /api/v1/auth/login` - Admin login
- `POST /api/v1/config` - Save chart configuration

### Desktop Client (API Key Auth)
- `GET /api/v1/sync/tickers` - List available tickers
- `GET /api/v1/sync/{exchange}/{symbol}/latest` - Latest timestamp
- `GET /api/v1/sync/{exchange}/{symbol}/trades` - Raw trades
- `GET /api/v1/sync/{exchange}/{symbol}/bars` - Footprint bars

## Memory Optimization (f1-micro)

The server is optimized for 0.6GB RAM:
- Single worker thread
- 10,000 trades max per ticker in memory
- Background persistence every 30 seconds
- 7-day automatic data cleanup

## Monitoring

### Health Check Endpoint
```bash
curl http://localhost:8080/api/v1/health
# Returns: {"status":"ok","uptime_seconds":12345}
```

### Detailed Health
```bash
curl http://localhost:8080/api/v1/health/detailed
```

## Troubleshooting

### Server won't start
```bash
# Check logs
sudo journalctl -u tick-collector -n 50

# Check port availability
sudo netstat -tlnp | grep 8080

# Check permissions
ls -la /opt/tick-collector/
```

### High memory usage
```bash
# Check memory
free -h

# Restart service
sudo systemctl restart tick-collector
```

### No data being collected
1. Check admin has assigned tickers to chart panes
2. Check WebSocket connections in logs
3. Verify exchange API accessibility

## Security Notes

- Admin password is hashed (bcrypt)
- API keys are randomly generated
- Session tokens expire after 24 hours
- Service runs as non-root user
- Systemd security hardening enabled

## Backup

```bash
# Backup data directory
tar -czvf tick-collector-backup.tar.gz /opt/tick-collector/data/

# Restore
sudo tar -xzvf tick-collector-backup.tar.gz -C /
sudo chown -R tick-collector:tick-collector /opt/tick-collector/data/
```
