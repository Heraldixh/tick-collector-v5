#!/bin/bash
# =============================================================================
# GCP FREE TIER VM PROVISIONING - Tick Collector Web Dashboard
# =============================================================================
# Cost: $0/month (always-free f1-micro with Debian Linux)
# RAM: 0.6GB (sufficient for actix-web server)
# Disk: 30GB Standard PD (free tier)
# =============================================================================

set -e

PROJECT_ID="${GCP_PROJECT_ID:-your-project-id}"
ZONE="us-west1-b"  # Free tier eligible zone
VM_NAME="tick-collector-web"

echo "🌐 Tick Collector Web - GCP Free Tier Deployment"
echo "================================================="
echo "VM: f1-micro (ALWAYS FREE)"
echo "Zone: $ZONE"
echo "Project: $PROJECT_ID"
echo ""

# Check if gcloud is installed
if ! command -v gcloud &> /dev/null; then
    echo "❌ gcloud CLI not found. Install from: https://cloud.google.com/sdk/docs/install"
    exit 1
fi

# Set project
gcloud config set project "$PROJECT_ID"

# Create firewall rule for HTTP (port 8080)
echo "🔥 Configuring firewall..."
gcloud compute firewall-rules describe allow-http-8080 &>/dev/null || \
gcloud compute firewall-rules create allow-http-8080 \
    --direction=INGRESS \
    --priority=1000 \
    --network=default \
    --action=ALLOW \
    --rules=tcp:8080 \
    --source-ranges=0.0.0.0/0 \
    --target-tags=web-server \
    --description="Allow HTTP traffic on port 8080"

# Create the VM
echo "🖥️  Creating f1-micro VM (FREE TIER)..."
gcloud compute instances create "$VM_NAME" \
    --machine-type=f1-micro \
    --zone="$ZONE" \
    --image-family=debian-12 \
    --image-project=debian-cloud \
    --boot-disk-size=30GB \
    --boot-disk-type=pd-standard \
    --tags=web-server \
    --metadata=startup-script='#!/bin/bash
# Update system
apt-get update
apt-get install -y curl build-essential pkg-config libssl-dev

# Install Rust
curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env

# Create app directories
mkdir -p /opt/tick-collector/{bin,static,data}
chown -R $USER:$USER /opt/tick-collector

echo "VM setup complete!"
'

# Reserve static IP (free with f1-micro)
echo "🌐 Reserving static IP..."
gcloud compute addresses create tick-collector-ip \
    --region="${ZONE%-*}" \
    --network-tier=STANDARD 2>/dev/null || true

STATIC_IP=$(gcloud compute addresses describe tick-collector-ip \
    --region="${ZONE%-*}" \
    --format="value(address)" 2>/dev/null || echo "pending")

# Assign static IP to VM
gcloud compute instances delete-access-config "$VM_NAME" \
    --zone="$ZONE" \
    --access-config-name="external-nat" 2>/dev/null || true

gcloud compute instances add-access-config "$VM_NAME" \
    --zone="$ZONE" \
    --address="$STATIC_IP" \
    --network-tier=STANDARD 2>/dev/null || true

echo ""
echo "✅ VM Created Successfully!"
echo ""
echo "📋 Details:"
echo "   VM Name: $VM_NAME"
echo "   Zone: $ZONE"
echo "   Static IP: $STATIC_IP"
echo ""
echo "📋 Next Steps:"
echo "1. Wait 2-3 minutes for startup script to complete"
echo "2. Run: ./2-deploy-app.sh"
echo "3. Access: http://$STATIC_IP:8080"
echo ""
echo "💰 Monthly Cost: \$0.00 (f1-micro is always free)"
