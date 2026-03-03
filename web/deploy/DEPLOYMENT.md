# Tick Collector Web Server - GCP Free Tier Deployment Guide

Complete step-by-step guide to deploy the Tick Collector server on Google Cloud Platform Free Tier.

---

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Step 1: Create GCP Account & Project](#step-1-create-gcp-account--project)
3. [Step 2: Create VM Instance](#step-2-create-vm-instance)
4. [Step 3: Configure Firewall](#step-3-configure-firewall)
5. [Step 4: Connect to VM](#step-4-connect-to-vm)
6. [Step 5: Install Dependencies](#step-5-install-dependencies)
7. [Step 6: Clone Repository](#step-6-clone-repository)
8. [Step 7: Build the Server](#step-7-build-the-server)
9. [Step 8: Install as Service](#step-8-install-as-service)
10. [Step 9: First-Time Setup](#step-9-first-time-setup)
11. [Management Commands](#management-commands)
12. [Troubleshooting](#troubleshooting)

---

## Prerequisites

- Google account
- Credit card (for verification, won't be charged on Free Tier)
- Git installed on your local machine

---

## Step 1: Create GCP Account & Project

### 1.1 Sign up for Google Cloud

1. Go to [https://cloud.google.com/free](https://cloud.google.com/free)
2. Click **"Get started for free"**
3. Sign in with your Google account
4. Complete billing setup (required but won't charge for Free Tier)
5. You get **$300 free credits** for 90 days + **Always Free** tier

### 1.2 Create a New Project

1. Go to [GCP Console](https://console.cloud.google.com)
2. Click the project dropdown (top left)
3. Click **"New Project"**
4. Name: `tick-collector` (or your choice)
5. Click **"Create"**
6. Select the new project from the dropdown

---

## Step 2: Create VM Instance

### 2.1 Navigate to Compute Engine

1. In GCP Console, go to **Navigation Menu** (☰) → **Compute Engine** → **VM instances**
2. If prompted, click **"Enable"** to enable Compute Engine API
3. Wait for it to initialize (1-2 minutes)

### 2.2 Create the VM

1. Click **"Create Instance"**

2. Configure the instance:

| Setting | Value |
|---------|-------|
| **Name** | `tick-collector-server` |
| **Region** | `us-west1`, `us-central1`, or `us-east1` (Free Tier eligible) |
| **Zone** | Any available zone |
| **Machine type** | `e2-micro` (2 vCPU, 1GB RAM) - **Free Tier eligible** |

3. **Boot disk** - Click **"Change"**:
   - Operating system: **Debian**
   - Version: **Debian GNU/Linux 11 (bullseye)**
   - Boot disk type: **Standard persistent disk**
   - Size: **30 GB** (Free Tier allows up to 30GB)
   - Click **"Select"**

4. **Firewall**:
   - ✅ Check **"Allow HTTP traffic"**
   - ✅ Check **"Allow HTTPS traffic"**

5. Click **"Create"**

6. Wait for the VM to start (green checkmark appears)

### 2.3 Note Your External IP

After creation, note the **External IP** address (e.g., `35.xxx.xxx.xxx`). You'll need this to access your server.

---

## Step 3: Configure Firewall

### 3.1 Create Firewall Rule for Port 8080

1. Go to **Navigation Menu** (☰) → **VPC Network** → **Firewall**
2. Click **"Create Firewall Rule"**

3. Configure:

| Setting | Value |
|---------|-------|
| **Name** | `allow-tick-collector` |
| **Network** | `default` |
| **Priority** | `1000` |
| **Direction** | `Ingress` |
| **Action** | `Allow` |
| **Targets** | `All instances in the network` |
| **Source filter** | `IPv4 ranges` |
| **Source IPv4 ranges** | `0.0.0.0/0` |
| **Protocols and ports** | Select **"Specified protocols and ports"** |
| | ✅ TCP: `8080` |

4. Click **"Create"**

---

## Step 4: Connect to VM

### 4.1 SSH via Browser

1. Go to **Compute Engine** → **VM instances**
2. Click **"SSH"** button next to your instance
3. A browser terminal window opens

### 4.2 Alternative: SSH from Terminal

```bash
# Install gcloud CLI first, then:
gcloud compute ssh tick-collector-server --zone=YOUR_ZONE
```

---

## Step 5: Install Dependencies

Run these commands in the SSH terminal:

```bash
# Update system packages
sudo apt update && sudo apt upgrade -y

# Install essential build tools
sudo apt install -y build-essential pkg-config libssl-dev git curl

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# Load Rust environment
source $HOME/.cargo/env

# Verify installation
rustc --version
cargo --version
```

---

## Step 6: Clone Repository

```bash
# Clone the repository
git clone https://github.com/YOUR_USERNAME/tick-collector-v5.git

# Navigate to web server directory
cd tick-collector-v5/web
```

**Replace `YOUR_USERNAME` with your actual GitHub username.**

---

## Step 7: Build the Server

```bash
# Build in release mode (optimized)
cargo build --release

# This takes 5-10 minutes on e2-micro
# You'll see "Finished release [optimized] target(s)"
```

**Note:** The first build takes longer due to dependency compilation.

---

## Step 8: Install as Service

### 8.1 Create Installation Directory

```bash
# Create directories
sudo mkdir -p /opt/tick-collector/data
sudo mkdir -p /opt/tick-collector/static

# Copy binary
sudo cp target/release/tick-collector-web /opt/tick-collector/

# Copy static files
sudo cp -r static/* /opt/tick-collector/static/

# Create service user
sudo useradd -r -s /bin/false tick-collector

# Set permissions
sudo chown -R tick-collector:tick-collector /opt/tick-collector
sudo chmod +x /opt/tick-collector/tick-collector-web
```

### 8.2 Create Systemd Service

```bash
sudo tee /etc/systemd/system/tick-collector.service > /dev/null << 'EOF'
[Unit]
Description=Tick Collector Web Server
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=tick-collector
Group=tick-collector
WorkingDirectory=/opt/tick-collector
ExecStart=/opt/tick-collector/tick-collector-web
Restart=always
RestartSec=5
Environment=RUST_LOG=info

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/tick-collector/data
PrivateTmp=true

[Install]
WantedBy=multi-user.target
EOF
```

### 8.3 Enable and Start Service

```bash
# Reload systemd
sudo systemctl daemon-reload

# Enable service (start on boot)
sudo systemctl enable tick-collector

# Start service
sudo systemctl start tick-collector

# Check status
sudo systemctl status tick-collector
```

You should see **"active (running)"** in green.

---

## Step 9: First-Time Setup

### 9.1 Access the Dashboard

1. Open your browser
2. Go to: `http://YOUR_EXTERNAL_IP:8080`
3. Replace `YOUR_EXTERNAL_IP` with your VM's external IP

### 9.2 Create Admin Account

1. You'll see the **"Welcome to Tick Collector"** setup screen
2. Enter a strong password
3. Click **"Create Admin Account"**

### 9.3 Login and Configure

1. Login with your password
2. Go to the **Charts** tab
3. Click on any chart pane
4. Search and select a ticker (e.g., `BTCUSDT`)
5. Data collection starts automatically!

### 9.4 Get API Key (for Desktop Client)

1. Go to the **API** tab
2. Copy the API key
3. Use this in your desktop client with header: `X-API-Key: your-key`

---

## Management Commands

### Service Control

```bash
# Start/Stop/Restart
sudo systemctl start tick-collector
sudo systemctl stop tick-collector
sudo systemctl restart tick-collector

# Check status
sudo systemctl status tick-collector

# Enable/Disable auto-start
sudo systemctl enable tick-collector
sudo systemctl disable tick-collector
```

### View Logs

```bash
# Follow logs in real-time
sudo journalctl -u tick-collector -f

# View last 100 lines
sudo journalctl -u tick-collector -n 100

# View today's logs
sudo journalctl -u tick-collector --since today

# View logs with timestamps
sudo journalctl -u tick-collector --since "1 hour ago"
```

### Check Disk Usage

```bash
# Check data directory size
du -sh /opt/tick-collector/data/

# Check individual directories
du -sh /opt/tick-collector/data/footprint/
du -sh /opt/tick-collector/data/trades/
```

### Update the Server

```bash
# Pull latest code
cd ~/tick-collector-v5/web
git pull origin main

# Rebuild
cargo build --release

# Stop service, update binary, restart
sudo systemctl stop tick-collector
sudo cp target/release/tick-collector-web /opt/tick-collector/
sudo cp -r static/* /opt/tick-collector/static/
sudo systemctl start tick-collector
```

---

## Troubleshooting

### Server Won't Start

```bash
# Check logs for errors
sudo journalctl -u tick-collector -n 50

# Check if port is in use
sudo netstat -tlnp | grep 8080

# Check permissions
ls -la /opt/tick-collector/
```

### Can't Access from Browser

1. Verify firewall rule exists for port 8080
2. Check VM external IP is correct
3. Ensure service is running: `sudo systemctl status tick-collector`

### High Memory Usage

```bash
# Check memory
free -h

# Restart service (clears memory)
sudo systemctl restart tick-collector
```

### No Data Being Collected

1. Login to admin dashboard
2. Check that tickers are assigned to chart panes
3. Check logs for WebSocket connection errors

---

## Free Tier Limits

| Resource | Free Tier Limit | Our Usage |
|----------|-----------------|-----------|
| VM | 1 e2-micro instance | ✅ 1 instance |
| Region | us-west1, us-central1, us-east1 | ✅ Any of these |
| Disk | 30 GB standard | ✅ 30 GB |
| Network | 1 GB egress/month | ⚠️ Monitor usage |

**Important:** Network egress beyond 1GB/month will incur charges. The server is optimized for minimal bandwidth.

---

## Security Recommendations

1. **Change default port** (optional): Edit service file to use a different port
2. **Use HTTPS**: Set up a reverse proxy with Let's Encrypt
3. **Restrict IP access**: Modify firewall rule to allow only your IP
4. **Regular updates**: Keep the system and application updated

---

## Backup & Restore

### Backup

```bash
# Backup data directory
tar -czvf tick-collector-backup-$(date +%Y%m%d).tar.gz /opt/tick-collector/data/

# Download backup (from your local machine)
gcloud compute scp tick-collector-server:~/tick-collector-backup-*.tar.gz ./
```

### Restore

```bash
# Upload backup to server
gcloud compute scp ./tick-collector-backup.tar.gz tick-collector-server:~/

# Restore on server
sudo systemctl stop tick-collector
sudo tar -xzvf ~/tick-collector-backup.tar.gz -C /
sudo chown -R tick-collector:tick-collector /opt/tick-collector/data/
sudo systemctl start tick-collector
```

---

## Quick Reference

| Item | Value |
|------|-------|
| **Dashboard URL** | `http://YOUR_IP:8080` |
| **Health Check** | `http://YOUR_IP:8080/api/v1/health` |
| **Config File** | `/opt/tick-collector/data/config.json` |
| **API Key File** | `/opt/tick-collector/data/api_key.txt` |
| **Logs** | `sudo journalctl -u tick-collector -f` |
| **Service File** | `/etc/systemd/system/tick-collector.service` |

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
