#!/bin/bash

# Background service to update Tor descriptors every hour
# This allows Tor to skip descriptor downloads on bootstrap
# Run this script with cron or systemd timer

# Configuration
CONTROL_PORT="9992"
CONTROL_PASSWORD="password1234_"
#LOG_FILE="~/code/eltord/tmp/prod/client/descriptor_update.log"


# Timestamp
# echo "[$(date '+%Y-%m-%d %H:%M:%S')] Starting descriptor update..." >> "$LOG_FILE"

# Send RELOAD signal to Tor (forces descriptor refresh)
echo -e "AUTHENTICATE \"$CONTROL_PASSWORD\"\r\nSIGNAL RELOAD\r\nQUIT\r\n" | nc localhost $CONTROL_PORT

if [ $? -eq 0 ]; then
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] ✅ Descriptor update successful"
else
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] ❌ Descriptor update failed"
fi

# Alternative: Force descriptor download via NEWNYM
# echo -e "AUTHENTICATE \"$CONTROL_PASSWORD\"\r\nSIGNAL NEWNYM\r\nQUIT\r\n" | nc localhost $CONTROL_PORT

# Log cache info
CACHE_DIR="~/code/eltord/tmp/prod/client"
if [ -f "$CACHE_DIR/cached-microdesc-consensus" ]; then
    CACHE_AGE=$(stat -f "%Sm" -t "%Y-%m-%d %H:%M:%S" "$CACHE_DIR/cached-microdesc-consensus")
    CACHE_SIZE=$(du -h "$CACHE_DIR/cached-microdesc-consensus" | cut -f1)
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] Cache: $CACHE_SIZE, last modified: $CACHE_AGE"
fi
