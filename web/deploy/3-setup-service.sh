#!/bin/bash
# =============================================================================
# SETUP SYSTEMD SERVICE AND MONITORING
# =============================================================================

set -e

ZONE="us-west1-b"
VM_NAME="tick-collector-web"

echo "🔧 Setting up maintenance and monitoring..."

gcloud compute ssh "$VM_NAME" --zone="$ZONE" --command='
# Create log rotation config
sudo tee /etc/logrotate.d/tick-collector > /dev/null <<EOF
/var/log/tick-collector/*.log {
    daily
    rotate 7
    compress
    delaycompress
    missingok
    notifempty
    create 644 root root
}
EOF

# Create health check script
sudo tee /usr/local/bin/tick-collector-health.sh > /dev/null <<EOF
#!/bin/bash
# Check if service is running
if ! systemctl is-active --quiet tick-collector-web; then
    echo "\$(date): Service down, restarting..."
    systemctl restart tick-collector-web
fi

# Check if HTTP is responding
if ! curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/api/v1/health | grep -q "200"; then
    echo "\$(date): HTTP not responding, restarting..."
    systemctl restart tick-collector-web
fi

# Check memory usage
MEM_USED=\$(free | grep Mem | awk "{print \$3/\$2 * 100.0}")
if (( \$(echo "\$MEM_USED > 85" | bc -l) )); then
    echo "\$(date): High memory usage: \${MEM_USED}%, restarting..."
    systemctl restart tick-collector-web
fi
EOF
sudo chmod +x /usr/local/bin/tick-collector-health.sh

# Setup cron for health checks (every 5 minutes)
(crontab -l 2>/dev/null | grep -v tick-collector-health; echo "*/5 * * * * /usr/local/bin/tick-collector-health.sh >> /var/log/tick-collector/health.log 2>&1") | crontab -

# Create log directory
sudo mkdir -p /var/log/tick-collector
sudo chown root:root /var/log/tick-collector

echo "✅ Maintenance setup complete!"
'

echo ""
echo "✅ Service and monitoring configured!"
echo ""
echo "📋 Scheduled Tasks:"
echo "  • Health check: Every 5 minutes"
echo "  • Log rotation: Daily (keeps 7 days)"
echo "  • Auto-restart: On crash or high memory"
