#!/bin/bash
# =============================================================================
# DEPLOY TICK COLLECTOR WEB TO GCP VM
# =============================================================================
# Cross-compile for Linux and deploy to f1-micro VM
# =============================================================================

set -e

ZONE="us-west1-b"
VM_NAME="tick-collector-web"

echo "🔨 Building Tick Collector Web for Linux..."

# Cross-compile for Linux (from Windows/Mac)
# First, add the target: rustup target add x86_64-unknown-linux-gnu
if cargo build --release -p tick-collector-web --target x86_64-unknown-linux-gnu 2>/dev/null; then
    BINARY_PATH="target/x86_64-unknown-linux-gnu/release/tick-collector-web"
else
    echo "⚠️  Cross-compilation failed. Building on VM instead..."
    BINARY_PATH=""
fi

echo "📦 Uploading files to VM..."

# Upload static files
gcloud compute scp --recurse web/static "$VM_NAME:/opt/tick-collector/" --zone="$ZONE"

if [ -n "$BINARY_PATH" ] && [ -f "$BINARY_PATH" ]; then
    # Upload pre-built binary
    gcloud compute scp "$BINARY_PATH" "$VM_NAME:/opt/tick-collector/bin/tick-collector-web" --zone="$ZONE"
else
    # Upload source and build on VM
    echo "📤 Uploading source code..."
    gcloud compute scp --recurse web/src "$VM_NAME:/tmp/tick-collector-src/" --zone="$ZONE"
    gcloud compute scp web/Cargo.toml "$VM_NAME:/tmp/tick-collector-src/" --zone="$ZONE"
    
    echo "🔨 Building on VM (this may take a few minutes)..."
    gcloud compute ssh "$VM_NAME" --zone="$ZONE" --command='
        source $HOME/.cargo/env
        cd /tmp/tick-collector-src
        cargo build --release
        cp target/release/tick-collector-web /opt/tick-collector/bin/
    '
fi

echo "🔧 Installing service..."
gcloud compute ssh "$VM_NAME" --zone="$ZONE" --command='
    sudo chmod +x /opt/tick-collector/bin/tick-collector-web
    
    # Create systemd service
    sudo tee /etc/systemd/system/tick-collector-web.service > /dev/null <<EOF
[Unit]
Description=Tick Collector Web Dashboard
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/tick-collector
Environment="CARGO_MANIFEST_DIR=/opt/tick-collector"
ExecStart=/opt/tick-collector/bin/tick-collector-web
Restart=always
RestartSec=5

# Memory limits for f1-micro (0.6GB RAM)
MemoryMax=400M
MemoryHigh=350M

[Install]
WantedBy=multi-user.target
EOF

    # Enable and start service
    sudo systemctl daemon-reload
    sudo systemctl enable tick-collector-web
    sudo systemctl restart tick-collector-web
    
    echo ""
    echo "Service status:"
    sudo systemctl status tick-collector-web --no-pager || true
'

# Get static IP
STATIC_IP=$(gcloud compute instances describe "$VM_NAME" \
    --zone="$ZONE" \
    --format="value(networkInterfaces[0].accessConfigs[0].natIP)")

echo ""
echo "✅ Deployment Complete!"
echo ""
echo "🌐 Dashboard URL: http://$STATIC_IP:8080"
echo ""
echo "📋 Useful Commands:"
echo "  View logs:    gcloud compute ssh $VM_NAME --zone=$ZONE --command='journalctl -u tick-collector-web -f'"
echo "  Check status: gcloud compute ssh $VM_NAME --zone=$ZONE --command='systemctl status tick-collector-web'"
echo "  Restart:      gcloud compute ssh $VM_NAME --zone=$ZONE --command='sudo systemctl restart tick-collector-web'"
