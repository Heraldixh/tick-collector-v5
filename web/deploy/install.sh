#!/bin/bash
# Tick Collector Web Server - GCP Free Tier Installation Script
# Designed for Debian/Ubuntu on f1-micro instance

set -e

echo "═══════════════════════════════════════════════════════════"
echo "🚀 Tick Collector Web Server - Installation"
echo "═══════════════════════════════════════════════════════════"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "❌ Please run as root (sudo ./install.sh)"
    exit 1
fi

# Create user and directories
echo "📁 Creating user and directories..."
useradd -r -s /bin/false tick-collector 2>/dev/null || true
mkdir -p /opt/tick-collector/data/trades
mkdir -p /opt/tick-collector/data/footprint
mkdir -p /opt/tick-collector/static

# Copy binary and static files
echo "📦 Copying files..."
cp tick-collector-web /opt/tick-collector/
cp -r static/* /opt/tick-collector/static/
chmod +x /opt/tick-collector/tick-collector-web

# Set ownership
chown -R tick-collector:tick-collector /opt/tick-collector

# Install systemd service
echo "⚙️ Installing systemd service..."
cp tick-collector.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable tick-collector

# Configure firewall (if ufw is installed)
if command -v ufw &> /dev/null; then
    echo "🔥 Configuring firewall..."
    ufw allow 8080/tcp
fi

echo ""
echo "═══════════════════════════════════════════════════════════"
echo "✅ Installation complete!"
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "Commands:"
echo "  Start:   sudo systemctl start tick-collector"
echo "  Stop:    sudo systemctl stop tick-collector"
echo "  Status:  sudo systemctl status tick-collector"
echo "  Logs:    sudo journalctl -u tick-collector -f"
echo ""
echo "Dashboard: http://YOUR_SERVER_IP:8080"
echo ""
